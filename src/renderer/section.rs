/**
 * Copyright 2019-2022, Benjamin Vaisvil and the zenith contributors
 */
use tui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use tui::style::{Color, Modifier, Style};
use tui::text::{Span, Spans};
use tui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use tui::Frame;

use super::{FromPrimitive, Render, ZBackend};
use std::collections::HashSet;
use std::fmt;

#[derive(FromPrimitive, PartialEq, Copy, Clone, Debug, Ord, PartialOrd, Eq)]
pub enum Section {
    Cpu = 0,
    Network = 1,
    Disk = 2,
    Graphics = 3,
    Process = 4,
}

impl fmt::Display for Section {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let name = match self {
            Section::Cpu => " CPU",
            Section::Disk => " Disk",
            Section::Graphics => " Graphics",
            Section::Network => " Network",
            Section::Process => " Process",
        };
        write!(f, "{}", name)
    }
}

pub fn sum_section_heights(geometry: &[(Section, f64)]) -> f64 {
    let mut sum = 0.0;
    for section in geometry {
        sum += section.1;
    }
    sum
}

pub struct SectionMGRList<'a> {
    pub items: Vec<(Section, ListItem<'a>)>,
    pub state: ListState,
}

impl<'a> SectionMGRList<'a> {
    pub fn with_geometry(geometry: Vec<(Section, f64)>) -> SectionMGRList<'a> {
        info!("Geometry: {:?}", geometry);
        info!("Geometry Len: {:?}", geometry.len());
        let mut section_set = HashSet::new();

        for (s, _) in geometry {
            section_set.insert(format!("{}", s));
        }

        debug!("Section Set: {:?}", section_set.len());
        debug!("Section Set: {:?}", section_set);
        let mut state = ListState::default();
        let items: Vec<(Section, ListItem)> = [0, 1, 2, 3, 4]
            .iter()
            .map(|i| {
                let section: Section = FromPrimitive::from_u32(*i as u32)
                    .expect("Index not in range for Section enum");
                let s: String = format!("{}", section);
                if section_set.contains(s.as_str()) {
                    (
                        section,
                        Span::styled(
                            format!("*{}", s),
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                    )
                } else {
                    (section, Span::styled(format!(" {}", s), Style::default()))
                }
            })
            .map(|(s, span)| (s, ListItem::new(span)))
            .collect();
        state.select(Some(0));
        SectionMGRList { items, state }
    }

    pub fn selected(&self) -> Option<Section> {
        self.state.selected().map(|s| self.items[s].0)
    }
}

pub fn render_section_mgr(list: &mut SectionMGRList<'_>, area: Rect, f: &mut Frame<'_, ZBackend>) {
    debug!("Rendering Section Manager");

    let layout = Layout::default()
        .margin(5)
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(1),
                Constraint::Percentage(80),
                Constraint::Length(5),
            ]
            .as_ref(),
        )
        .split(area);
    let header_style = Style::default().fg(Color::Green);
    let t = vec![Span::styled("Options", header_style)];
    let help = vec![Span::styled(
        "Navigate [↑/↓] Toggle [Space] Return [F1]",
        header_style,
    )];
    Paragraph::new(Spans::from(t))
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Center)
        .render(f, layout[0]);
    Paragraph::new(Spans::from(help))
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Center)
        .render(f, layout[2]);
    let list_items: Vec<ListItem> = list.items.iter().map(|i| i.1.clone()).collect();
    let list_widget = List::new(list_items)
        .block(Block::default().title("Sections").borders(Borders::ALL))
        .highlight_style(Style::default().bg(Color::Green))
        .highlight_symbol("➡ ");
    f.render_stateful_widget(list_widget, layout[1], &mut list.state);
}
