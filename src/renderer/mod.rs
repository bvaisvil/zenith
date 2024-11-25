/**
 * Copyright 2019-2022, Benjamin Vaisvil and the zenith contributors
 */
mod cpu;
mod disk;
mod graphics;
mod help;
pub mod macros;
mod network;
mod process;
pub mod section;
pub mod style;
mod title;
use crate::metrics::graphics::device::GraphicsExt;
use crate::metrics::histogram::View;
use crate::metrics::zprocess::*;
use crate::metrics::*;
use crate::renderer::section::{sum_section_heights, Section, SectionMGRList};
use crate::util::*;
use crossterm::{
    event::{KeyCode as Key, KeyEvent, KeyModifiers},
    execute,
    terminal::EnterAlternateScreen,
};
use num_traits::FromPrimitive;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::cmp::Eq;
use std::io;
use std::io::Stdout;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use ratatui::backend::Backend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

const PROCESS_SELECTION_GRACE: Duration = Duration::from_millis(2000);
const LEFT_PANE_WIDTH: u16 = 34u16;

type ZBackend = CrosstermBackend<Stdout>;

/// Compatibility trait, that preserves an older method from tui 0.6.5
/// Exists mostly to keep the caller code idiomatic for the use cases in this file
/// May be refactored out later if the widget usage patterns change
trait Render<B>
where
    B: Backend,
{
    fn render(self, f: &mut Frame<B>, area: Rect);
}

impl<T, B> Render<B> for T
where
    T: ratatui::widgets::Widget,
    B: Backend,
{
    fn render(self, f: &mut Frame<B>, area: Rect) {
        f.render_widget(self, area)
    }
}

macro_rules! update_section_height {
    ($x:expr, $val:expr) => {
        if $x + $val > 0.0 && $x + $val < 100.0 {
            $x += $val;
            true
        } else {
            false
        }
    };
}

#[derive(FromPrimitive, PartialEq, Copy, Clone, Debug, Ord, PartialOrd, Eq)]
pub enum FileSystemDisplay {
    Usage,
    Activity,
}

/// Returns rectangles for the left pane and right histogram, and a new view for the right histogram
fn split_left_right_pane(
    title: &str,
    area: Rect,
    f: &mut Frame<'_, ZBackend>,
    view: View,
    border_style: Style,
) -> (Rc<[Rect]>, View) {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style)
        .render(f, area);
    let layout = Layout::default()
        .margin(0)
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(LEFT_PANE_WIDTH), Constraint::Min(10)].as_ref())
        .split(area);

    let view = View {
        width: usize::from(layout[1].width).saturating_sub(2),
        ..view
    };

    (layout, view)
}

/// current size of the terminal returned as (columns, rows)
fn terminal_size() -> (u16, u16) {
    crossterm::terminal::size().expect("Failed to get terminal size")
}

/// ceil to nearest upper even number
macro_rules! ceil_even {
    ($x:expr) => {
        ($x + 1) / 2 * 2
    };
}

/// Convert percentage heights to length constraints. This is done since sections other
/// than process have two sub-parts and should be of even height.
fn eval_constraints(
    section_geometry: &[(Section, f64)],
    height: u16,
    borrowed: &mut bool,
) -> Vec<Constraint> {
    debug!("Get Constraints");
    let mut constraints = vec![Constraint::Length(1)];
    let avail_height = height as i32 - 1;
    let mut process_index = -1;
    let mut max_others = 0;
    let mut max_others_index = -1;
    let mut sum_others = 0;
    // each section should have a height of at least 2 rows
    let mut max_section_height = avail_height - section_geometry.len() as i32 * 2;
    // process section is at least 4 rows high
    if section_geometry.iter().any(|s| s.0 == Section::Process) {
        max_section_height -= 2;
    }
    // convert percentage heights to length constraints and apply additional
    // criteria that height should be even number for non-process sections
    for (section_index, section) in section_geometry.iter().enumerate() {
        let required_height = section.1 * avail_height as f64 / 100.0;
        // ensure max_section_height is at least 2 after every recalculation
        max_section_height = max_section_height.max(2);
        if section.0 == Section::Process {
            process_index = section_index as i32;
            constraints.push(Constraint::Min(4));
        } else {
            // round to nearest even size for the two sub-parts in each section display
            let section_height =
                max_section_height.min(ceil_even!(required_height.floor().max(1.0) as i32));
            sum_others += section_height;
            // adjust max_section_height for subsequent sections
            max_section_height -= section_height - 2;
            if section_height >= max_others {
                max_others = section_height;
                max_others_index = section_index as i32;
            }
            constraints.push(Constraint::Length(section_height as u16));
        }
    }
    // remaining is what will be actually used for process section but if its too small (due to
    // rounding to even heights for other sections), then borrow rows from the largest section
    if process_index != -1 {
        let process_height = avail_height - sum_others;
        if process_height < 4 && max_others > 4 {
            let borrow = ceil_even!(4 - process_height).min(max_others - 4);
            // (max_others - borrow) will be >= 4 due to the min() above so cast to u16 is safe
            constraints[max_others_index as usize + 1] =
                Constraint::Length((max_others - borrow) as u16);
            constraints[process_index as usize + 1] =
                Constraint::Min((process_height + borrow) as u16);
            *borrowed = true;
        } else {
            constraints[process_index as usize + 1] = Constraint::Min(process_height as u16);
        }
    }

    constraints
}

fn get_constraints(section_geometry: &[(Section, f64)], height: u16) -> Vec<Constraint> {
    let mut borrowed = false;
    eval_constraints(section_geometry, height, &mut borrowed)
}

pub struct TerminalRenderer<'a> {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    app: CPUTimeApp,
    events: Events,
    process_table_row_start: usize,
    gfx_device_index: usize,
    file_system_index: usize,
    file_system_display: FileSystemDisplay,
    /// Index in the vector below is "order" on the screen starting from the top
    /// (usually CPU) while value is the section it belongs to and its current height (as %).
    /// Currently all sections are stacked on top of one another horizontally and
    /// occupy entire width of the screen but this may change going forward. For the case
    /// where there are multiple sections stacked vertically, the "order" can have the
    /// convention of top-bottom and left-right in each horizontal layer and the width of
    /// each section be tracked below. For more generic positioning (e.g. sections cutting
    /// across others vertically), this mapping needs to also include the position of
    /// top-left corner of the section. In that case the only significance that the
    /// "order" will have is the sequence in which the TAB key will shift focus
    /// among the sections.
    section_geometry: Vec<(Section, f64)>,
    zoom_factor: u32,
    update_number: u32,
    hist_start_offset: usize,
    selected_section_index: usize,
    constraints: Vec<Constraint>,
    process_message: Option<String>,
    show_help: bool,
    show_paths: bool,
    show_find: bool,
    show_section_mgr: bool,
    filter: String,
    highlighted_row: usize,
    selection_grace_start: Option<Instant>,
    section_manager_options: SectionMGRList<'a>,
    disable_history: bool,
    recompute_constraints_on_start_up: bool,
}

impl<'a> TerminalRenderer<'_> {
    pub fn new(
        tick_rate: u64,
        section_geometry: &[(Section, f64)],
        db_path: Option<PathBuf>,
        disable_history: bool,
    ) -> TerminalRenderer {
        debug!("Create Metrics App");
        let mut app = CPUTimeApp::new(Duration::from_millis(tick_rate), db_path);
        debug!("Create Event Loop");
        let events = Events::new(app.histogram_map.tick);

        debug!("Hide Cursor");
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen).expect("Unable to enter alternate screen");
        let backend = CrosstermBackend::new(stdout);
        let mut terminal =
            Terminal::new(backend).expect("Couldn't create new terminal with backend");
        terminal.hide_cursor().ok();

        let constraints = get_constraints(section_geometry, terminal_size().1);
        let mut section_geometry = section_geometry.to_vec();
        let mut recompute_constraints_on_start_up = false;
        app.update_gfx_devices();
        if app.gfx_devices.is_empty()
            && section_geometry
                .iter()
                .any(|(s, _)| *s == Section::Graphics)
        {
            section_geometry.retain(|(section, _)| *section != Section::Graphics);
            recompute_constraints_on_start_up = true;
        }
        TerminalRenderer {
            terminal,
            app,
            events,
            process_table_row_start: 0,
            gfx_device_index: 0,
            file_system_index: 0,
            file_system_display: FileSystemDisplay::Activity,
            section_geometry: section_geometry.clone(),
            zoom_factor: 1,
            update_number: 0,
            // select the last section by default (normally should be Process)
            selected_section_index: section_geometry.len() - 1,
            constraints,
            process_message: None,
            hist_start_offset: 0,
            show_help: false,
            show_paths: false,
            show_find: false,
            show_section_mgr: false,
            filter: String::from(""),
            highlighted_row: 0,
            selection_grace_start: None,
            section_manager_options: SectionMGRList::with_geometry(section_geometry),
            disable_history,
            recompute_constraints_on_start_up,
        }
    }

    /// Update section height by given delta value in number of rows.
    /// This transforms the value in terms of percentage and reduces the
    /// other section percentages proportionally. By this it means that
    /// larger sections will be reduced more while smaller ones will be
    /// reduced less. Overall the total percentage heights in section_geometry
    /// should always be close to 100%.
    async fn update_section_height(&mut self, delta: i16) {
        // convert val to percentage
        let (_, height) = terminal_size();
        let avail_height = (height - 1) as f64;
        let mut val = delta as f64 * 100.0 / avail_height;
        let selected_index = self.selected_section_index;
        let mut new_geometry = self.section_geometry.to_vec();
        if update_section_height!(new_geometry[selected_index].1, val) {
            // reduce proportionately from other sections if the value was updated
            let rest = 100.0 - new_geometry[selected_index].1 + val;
            for (section_index, section) in new_geometry.iter_mut().enumerate() {
                if section_index != selected_index {
                    let change = section.1 * val / rest;
                    // abort if limits are exceeded
                    if !update_section_height!(section.1, -change) {
                        val = 0.0; // abort changes
                        break;
                    }
                }
            }
            if val != 0.0 {
                let mut borrowed = false;
                let new_constraints = eval_constraints(&new_geometry, height, &mut borrowed);
                // abort if process section became too small and borrowed from others
                if !borrowed {
                    let new_sum_heights = sum_section_heights(&new_geometry);
                    assert!((99.9..=100.1).contains(&new_sum_heights));
                    self.section_geometry = new_geometry;
                    self.constraints = new_constraints;
                }
            }
        }
    }

    fn selected_section(&self) -> Section {
        self.section_geometry[self.selected_section_index].0
    }

    pub async fn start(&mut self) {
        debug!("Starting Main Loop.");
        let disable_history = self.disable_history;
        if self.recompute_constraints_on_start_up {
            self.recompute_constraints();
            self.recompute_constraints_on_start_up = false;
        }
        loop {
            let app = &self.app;
            let pst = &self.process_table_row_start;
            let mut width: u16 = 0;
            let mut process_table_height: u16 = 0;
            let zf = &self.zoom_factor;
            let constraints = &self.constraints;
            let geometry = &self.section_geometry.to_vec();
            let section_manager_options = &mut self.section_manager_options;
            let selected = self.section_geometry[self.selected_section_index].0;
            let process_message = &self.process_message;
            let offset = &self.hist_start_offset;
            let un = &self.update_number;
            let show_help = self.show_help;
            let show_section_mgr = self.show_section_mgr;
            let show_paths = self.show_paths;
            let filter = &self.filter;
            let show_find = self.show_find;
            let mut highlighted_process: Option<Box<ZProcess>> = None;
            let process_table = process::filter_process_table(app, &self.filter);
            let gfx_device_index = &self.gfx_device_index;
            let file_system_index = &self.file_system_index;
            let file_system_display = &self.file_system_display;

            if !process_table.is_empty() && self.highlighted_row >= process_table.len() {
                self.highlighted_row = process_table.len() - 1;
            }
            let highlighted_row = self.highlighted_row;

            self.terminal
                .draw(|f| {
                    width = f.size().width;
                    if show_help {
                        let v_sections = Layout::default()
                            .direction(Direction::Vertical)
                            .margin(0)
                            .constraints([Constraint::Length(1), Constraint::Length(40)].as_ref())
                            .split(f.size());

                        title::render_top_title_bar(app, v_sections[0], f, zf, offset);
                        let history_recording = match (app.writes_db_store(), disable_history) {
                            (true, _) => HistoryRecording::On,
                            (false, true) => HistoryRecording::UserDisabled,
                            (false, false) => HistoryRecording::OtherInstancePrevents,
                        };
                        help::render_help(app, v_sections[1], f, history_recording);
                    } else if show_section_mgr {
                        let v_sections = Layout::default()
                            .direction(Direction::Vertical)
                            .margin(0)
                            .constraints([Constraint::Length(1), Constraint::Length(40)].as_ref())
                            .split(f.size());
                        title::render_top_title_bar(app, v_sections[0], f, zf, offset);
                        section::render_section_mgr(section_manager_options, v_sections[1], f);
                    } else {
                        // create layouts
                        // primary vertical
                        let v_sections = Layout::default()
                            .direction(Direction::Vertical)
                            .margin(0)
                            .constraints(constraints.as_slice())
                            .split(f.size());

                        title::render_top_title_bar(app, v_sections[0], f, zf, offset);
                        let view = View {
                            zoom_factor: *zf,
                            update_number: *un,
                            width: 0,
                            offset: *offset,
                        };
                        for section_index in 0..geometry.len() {
                            let v_section = v_sections[section_index + 1];
                            let current_section = geometry[section_index].0;
                            let border_style = if current_section == selected {
                                Style::default().fg(Color::Red)
                            } else {
                                Style::default()
                            };
                            match current_section {
                                Section::Cpu => {
                                    cpu::render_cpu(app, v_section, f, view, border_style)
                                }
                                Section::Network => {
                                    network::render_net(app, v_section, f, view, border_style)
                                }
                                Section::Disk => disk::render_disk(
                                    app,
                                    v_section,
                                    f,
                                    view,
                                    border_style,
                                    file_system_index,
                                    file_system_display,
                                ),
                                Section::Graphics => graphics::render_graphics(
                                    app,
                                    v_section,
                                    f,
                                    view,
                                    gfx_device_index,
                                    border_style,
                                ),
                                Section::Process => {
                                    if let Some(p) = app.selected_process.as_ref() {
                                        process::render_process(
                                            app,
                                            v_section,
                                            f,
                                            border_style,
                                            process_message,
                                            p,
                                        );
                                    } else {
                                        highlighted_process = process::render_process_table(
                                            app,
                                            &process_table,
                                            v_section,
                                            *pst,
                                            f,
                                            border_style,
                                            show_paths,
                                            show_find,
                                            filter,
                                            highlighted_row,
                                        );
                                        if v_section.height > 4 {
                                            // account for table border & margins.
                                            process_table_height = v_section.height - 5;
                                        }
                                    }
                                }
                            }
                        }
                    }
                })
                .expect("Could not draw frame.");

            let event = self.events.next().expect("No new event.");
            let action = match event {
                Event::Input(input) => {
                    let process_table = process_table.into_owned();
                    self.process_key_event(
                        input,
                        &process_table,
                        process_table_height,
                        highlighted_process,
                    )
                    .await
                }
                Event::Resize(_, height) => {
                    self.constraints = get_constraints(&self.section_geometry, height);
                    Action::Continue
                }
                Event::Tick => {
                    self.process_tick().await;
                    Action::Continue
                }
                Event::Save => {
                    debug!("Event Save");
                    self.app.save_state().await;
                    Action::Continue
                }
                Event::Terminate => {
                    debug!("Event Terminate");
                    Action::Quit
                }
            };
            match action {
                Action::Quit => break,
                Action::Continue => {}
            }
        }
    }

    async fn process_tick(&mut self) {
        debug!("Event Tick");

        if self.app.selected_process.is_none() {
            if let Some(start) = self.selection_grace_start {
                if start.elapsed() > PROCESS_SELECTION_GRACE {
                    self.selection_grace_start = None;
                }
            }
        }

        let keep_order =
            self.app.selected_process.is_some() || self.selection_grace_start.is_some();

        self.app.update(keep_order).await;
        self.update_number += 1;
        if self.update_number == self.zoom_factor {
            self.update_number = 0;
        }
    }

    async fn process_key_event(
        &mut self,
        input: KeyEvent,
        process_table: &[i32],
        process_table_height: u16,
        highlighted_process: Option<Box<ZProcess>>,
    ) -> Action {
        debug!("Event Key: {:?}", input);
        match input.code {
            Key::Up => self.view_up(process_table, 1),
            Key::PageUp => self.view_up(process_table, process_table_height.into()),
            Key::Down => self.view_down(process_table, process_table_height.into(), 1),
            Key::PageDown => self.view_down(
                process_table,
                process_table_height.into(),
                process_table_height.into(),
            ),
            Key::Home => self.view_up(process_table, process_table.len()),
            Key::End => self.view_down(
                process_table,
                process_table_height.into(),
                process_table.len(),
            ),
            Key::Left => self.histogram_left(),
            Key::Right => self.histogram_right(),
            Key::Enter => self.select(highlighted_process),
            Key::Char('c') => {
                if input.modifiers.contains(KeyModifiers::CONTROL) {
                    return Action::Quit;
                } else if self.show_find {
                    self.process_find_input(input);
                }
            }
            _other => {
                if self.show_find {
                    self.process_find_input(input);
                } else {
                    return self.process_toplevel_input(input).await;
                }
            }
        };
        Action::Continue
    }

    fn select(&mut self, highlighted_process: Option<Box<ZProcess>>) {
        let selected = self.selected_section();
        if selected == Section::Process {
            self.app.select_process(highlighted_process);
            self.process_message = None;
            self.show_find = false;
            self.process_table_row_start = 0;
        }
    }

    fn view_up(&mut self, process_table: &[i32], delta: usize) {
        let selected = self.selected_section();
        if self.show_section_mgr {
            match self.section_manager_options.state.selected() {
                Some(i) => {
                    let mut idx = 0;
                    if (i as i32 - delta as i32) > 0 {
                        idx = i - delta;
                    }
                    self.section_manager_options.state.select(Some(idx));
                }
                None => self.section_manager_options.state.select(Some(0)),
            }
        } else if selected == Section::Graphics {
            if self.app.gfx_devices.len() == 0 {
                self.gfx_device_index = 0;
            } else if self.gfx_device_index > 0 {
                self.gfx_device_index -= 1;
            }
        } else if selected == Section::Disk {
            if self.file_system_index > 0 {
                self.file_system_index -= 1;
            }
        } else if selected == Section::Process {
            if self.app.selected_process.is_some() || process_table.is_empty() {
                return;
            }

            self.selection_grace_start = Some(Instant::now());
            if self.highlighted_row != 0 {
                self.highlighted_row = self.highlighted_row.saturating_sub(delta);
            }
            if self.process_table_row_start > 0
                && self.highlighted_row < self.process_table_row_start
            {
                self.process_table_row_start = self.process_table_row_start.saturating_sub(delta);
            }
        }
    }

    fn view_down(&mut self, process_table: &[i32], process_table_height: usize, delta: usize) {
        use std::cmp::min;
        let selected = self.selected_section();
        if self.show_section_mgr {
            match self.section_manager_options.state.selected() {
                Some(i) => {
                    let mut idx = self.section_manager_options.items.len() - 1;
                    if i + delta < idx {
                        idx = i + delta;
                    }
                    self.section_manager_options.state.select(Some(idx));
                }
                None => self.section_manager_options.state.select(Some(0)),
            }
        } else if selected == Section::Graphics {
            if self.app.gfx_devices.len() == 0 {
                self.gfx_device_index = 0;
            } else if self.gfx_device_index < self.app.gfx_devices.len() - 1 {
                self.gfx_device_index += 1;
            }
        } else if selected == Section::Disk {
            if self.file_system_index < self.app.disks.len() - 1 {
                self.file_system_index += 1;
            }
        } else if selected == Section::Process {
            if self.app.selected_process.is_some() || process_table.is_empty() {
                return;
            }

            self.selection_grace_start = Some(Instant::now());
            if self.highlighted_row < process_table.len() - 1 {
                self.highlighted_row = min(self.highlighted_row + delta, process_table.len() - 1);
            }
            if self.process_table_row_start < process_table.len()
                && self.highlighted_row > (self.process_table_row_start + process_table_height)
            {
                self.process_table_row_start = min(
                    self.process_table_row_start + delta,
                    process_table.len() - process_table_height - 1,
                );
            }
        }
    }

    fn histogram_left(&mut self) {
        if let Some(w) = self.app.histogram_map.histograms_width() {
            self.hist_start_offset += 1;
            if self.hist_start_offset > w + 1 {
                self.hist_start_offset = w - 1;
            }
        }
        self.hist_start_offset += 1;
    }

    fn histogram_right(&mut self) {
        if self.hist_start_offset > 0 {
            self.hist_start_offset -= 1;
        }
    }

    fn process_find_input(&mut self, input: KeyEvent) {
        match input.code {
            Key::Esc => {
                self.show_find = false;
                self.filter = String::from("");
            }
            Key::Char(c) if c != '\n' => {
                self.selection_grace_start = Some(Instant::now());
                self.filter.push(c)
            }
            Key::Delete => match self.filter.pop() {
                Some(_c) => {}
                None => self.show_find = false,
            },
            Key::Backspace => match self.filter.pop() {
                Some(_c) => {}
                None => self.show_find = false,
            },
            _ => {}
        }
    }

    fn recompute_constraints(&mut self) {
        self.selected_section_index = 0;
        for idx in 0..self.section_geometry.len() {
            self.section_geometry[idx].1 = 100.0 / self.section_geometry.len() as f64;
        }
        let new_geometry = self.section_geometry.clone();
        let selected = self.section_manager_options.state.selected();
        self.section_manager_options = SectionMGRList::with_geometry(new_geometry);
        self.section_manager_options.state.select(selected);
        self.constraints = get_constraints(self.section_geometry.as_slice(), terminal_size().1);
    }

    fn toggle_section(&mut self) {
        if self.show_section_mgr {
            if let Some(s) = self.section_manager_options.selected() {
                if self.section_geometry.len() > 1
                    && self.section_geometry.iter().any(|(gs, _)| *gs == s)
                {
                    self.section_geometry.retain(|(section, _)| *section != s);
                    self.recompute_constraints();
                } else if !self.section_geometry.iter().any(|(gs, _)| *gs == s) {
                    let idx = 0;
                    self.section_geometry.insert(idx, (s, 1.0));
                    self.section_geometry
                        .sort_by(|(a_section, _), (b_section, _)| {
                            a_section
                                .partial_cmp(b_section)
                                .expect("Can't compare sections. Shouldn't happen.")
                        });
                    self.recompute_constraints();
                }
            }
        }
    }

    fn toggle_section_mgr(&mut self) {
        self.show_section_mgr = !self.show_section_mgr;
    }

    async fn process_toplevel_input(&mut self, input: KeyEvent) -> Action {
        match input.code {
            Key::Char('q') => {
                return Action::Quit;
            }
            Key::Char('.') | Key::Char('>') => {
                if self.app.psortby == ProcessTableSortBy::Cmd {
                    self.app.psortby = ProcessTableSortBy::Pid;
                } else {
                    self.app.psortby = FromPrimitive::from_u32(self.app.psortby as u32 + 1)
                        .expect("invalid value to set psortby");
                }
                self.app.sort_process_table();
            }
            Key::Char(',') | Key::Char('<') => {
                if self.app.psortby == ProcessTableSortBy::Pid {
                    self.app.psortby = ProcessTableSortBy::Cmd;
                } else {
                    self.app.psortby = FromPrimitive::from_u32(self.app.psortby as u32 - 1)
                        .expect("invalid value to set psortby");
                }
                self.app.sort_process_table();
            }
            Key::Char(';') => {
                match self.app.psortorder {
                    ProcessTableSortOrder::Ascending => {
                        self.app.psortorder = ProcessTableSortOrder::Descending
                    }
                    ProcessTableSortOrder::Descending => {
                        self.app.psortorder = ProcessTableSortOrder::Ascending
                    }
                }
                self.app.sort_process_table();
            }
            Key::Char('+') | Key::Char('=') => {
                if self.zoom_factor > 1 {
                    self.zoom_factor -= 1;
                }
                self.update_number = 0;
            }
            Key::Char('-') => {
                if self.zoom_factor < 100 {
                    self.zoom_factor += 1;
                }
                self.update_number = 0;
            }
            Key::Esc | Key::Char('b') => {
                self.app.selected_process = None;
                self.process_message = None;
            }
            Key::Char('s') => {
                self.process_message = match &self.app.selected_process {
                    Some(p) => Some(p.suspend().await),
                    None => None,
                };
            }
            Key::Char('r') => {
                self.process_message = match &self.app.selected_process {
                    Some(p) => Some(p.resume().await),
                    None => None,
                };
            }
            Key::Char('k') => {
                self.process_message = match &self.app.selected_process {
                    Some(p) => Some(p.kill().await),
                    None => None,
                };
            }
            Key::Char('t') => {
                self.process_message = match &self.app.selected_process {
                    Some(p) => Some(p.terminate().await),
                    None => None,
                };
            }
            Key::Char('n') => {
                self.process_message = self.app.selected_process.as_mut().map(|p| p.nice());
            }
            Key::Char('p') if self.app.selected_process.is_some() => {
                self.process_message = self
                    .app
                    .selected_process
                    .as_mut()
                    .map(|p| p.set_priority(0));
            }
            k @ Key::Tab | k @ Key::BackTab => {
                // hopefully cross platform enough regarding https://github.com/crossterm-rs/crossterm/issues/442
                self.selected_section_index =
                    if k == Key::BackTab || input.modifiers.contains(KeyModifiers::SHIFT) {
                        match self.selected_section_index {
                            0 => self.section_geometry.len() - 1,
                            x => x - 1,
                        }
                    } else {
                        (self.selected_section_index + 1) % self.section_geometry.len()
                    };
            }
            Key::Char(' ') => {
                self.toggle_section();
            }
            Key::F(1) | Key::Char('i') => {
                self.toggle_section_mgr();
            }
            Key::Char('m') => {
                self.update_section_height(-2).await;
            }
            Key::Char('e') => {
                self.update_section_height(2).await;
            }
            Key::Char('`') => {
                self.zoom_factor = 1;
                self.hist_start_offset = 0;
            }
            Key::Char('h') => {
                self.show_help = !self.show_help;
            }
            Key::Char('p') => {
                self.show_paths = !self.show_paths;
            }
            Key::Char('/') => {
                self.show_find = true;
                self.highlighted_row = 0;
                self.process_table_row_start = 0;
            }
            Key::Char('a') => {
                if self.file_system_display == FileSystemDisplay::Activity {
                    self.file_system_display = FileSystemDisplay::Usage;
                } else {
                    self.file_system_display = FileSystemDisplay::Activity;
                }
            }
            _ => {}
        }

        Action::Continue
    }
}

#[must_use]
enum Action {
    Continue,
    Quit,
}

pub enum HistoryRecording {
    On,
    UserDisabled,
    OtherInstancePrevents,
}
