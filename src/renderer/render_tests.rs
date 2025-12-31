/**
 * Copyright 2019-2025, Benjamin Vaisvil and the zenith contributors
 * Integration tests using ratatui TestBackend for UI rendering
 */
use ratatui::backend::TestBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;

use crate::metrics::histogram::View;
use crate::renderer::section::Section;
use crate::renderer::HistoryRecording;

/// Helper to create a test terminal with given dimensions
fn create_test_terminal(width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    Terminal::new(backend).expect("Failed to create test terminal")
}

#[test]
fn test_terminal_backend_creation() {
    let terminal = create_test_terminal(80, 24);
    let size = terminal.size().unwrap();

    assert_eq!(size.width, 80);
    assert_eq!(size.height, 24);
}

#[test]
fn test_buffer_dimensions() {
    let terminal = create_test_terminal(100, 30);
    let buffer = terminal.backend().buffer();

    assert_eq!(buffer.area.width, 100);
    assert_eq!(buffer.area.height, 30);
}

#[test]
fn test_view_struct_creation() {
    let view = View {
        zoom_factor: 2,
        update_number: 5,
        width: 80,
        offset: 10,
    };

    assert_eq!(view.zoom_factor, 2);
    assert_eq!(view.update_number, 5);
    assert_eq!(view.width, 80);
    assert_eq!(view.offset, 10);
}

#[test]
fn test_section_ordering_in_render() {
    // Test that sections have a defined order for sorting
    assert!(Section::Cpu < Section::Network);
    assert!(Section::Network < Section::Disk);
    assert!(Section::Disk < Section::Graphics);
    assert!(Section::Graphics < Section::Process);
}

#[test]
fn test_section_equality_in_render() {
    let cpu1 = Section::Cpu;
    let cpu2 = Section::Cpu;
    let net = Section::Network;

    assert_eq!(cpu1, cpu2);
    assert_ne!(cpu1, net);
}

#[test]
fn test_history_recording_variants() {
    // Test that all HistoryRecording variants can be created
    let _on = HistoryRecording::On;
    let _disabled = HistoryRecording::UserDisabled;
    let _prevented = HistoryRecording::OtherInstancePrevents;
}

#[test]
fn test_default_border_style() {
    let style = Style::default();
    // Default style should have no modifications
    assert_eq!(style, Style::default());
}

#[test]
fn test_terminal_draw_closure() {
    let mut terminal = create_test_terminal(80, 24);

    terminal
        .draw(|frame| {
            let area = frame.area();
            assert_eq!(area.width, 80);
            assert_eq!(area.height, 24);
        })
        .expect("Failed to draw");
}

#[test]
fn test_rect_creation() {
    let rect = Rect::new(0, 0, 80, 24);

    assert_eq!(rect.x, 0);
    assert_eq!(rect.y, 0);
    assert_eq!(rect.width, 80);
    assert_eq!(rect.height, 24);
}

#[test]
fn test_rect_area_calculation() {
    let rect = Rect::new(0, 0, 10, 5);
    assert_eq!(rect.area(), 50);
}

#[test]
fn test_small_terminal_handling() {
    // Test that we can handle very small terminals
    let terminal = create_test_terminal(20, 5);
    let size = terminal.size().unwrap();

    assert_eq!(size.width, 20);
    assert_eq!(size.height, 5);
}

#[test]
fn test_large_terminal_handling() {
    // Test large terminal dimensions
    let terminal = create_test_terminal(200, 60);
    let size = terminal.size().unwrap();

    assert_eq!(size.width, 200);
    assert_eq!(size.height, 60);
}

#[test]
fn test_layout_split_vertical() {
    let mut terminal = create_test_terminal(80, 24);

    terminal
        .draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0)])
                .split(frame.area());

            // First chunk should be 1 row for title
            assert_eq!(chunks[0].height, 1);
            // Second chunk should take remaining space
            assert_eq!(chunks[1].height, 23);
        })
        .unwrap();
}

#[test]
fn test_layout_split_horizontal() {
    let mut terminal = create_test_terminal(80, 24);

    terminal
        .draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(frame.area());

            assert_eq!(chunks[0].width, 40);
            assert_eq!(chunks[1].width, 40);
        })
        .unwrap();
}

#[test]
fn test_block_with_borders() {
    let mut terminal = create_test_terminal(40, 10);

    terminal
        .draw(|frame| {
            let block = Block::default().title("Test Block").borders(Borders::ALL);

            frame.render_widget(block, frame.area());
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    // Check corners have border characters
    assert_ne!(buffer[(0, 0)].symbol(), " ");
}

#[test]
fn test_paragraph_rendering() {
    let mut terminal = create_test_terminal(40, 5);

    terminal
        .draw(|frame| {
            let para = Paragraph::new("Hello, World!");
            frame.render_widget(para, frame.area());
        })
        .unwrap();

    // Buffer should contain the text
    let buffer = terminal.backend().buffer();
    let mut found = false;
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            if buffer[(x, y)].symbol() == "H" {
                found = true;
                break;
            }
        }
    }
    assert!(found, "Should find 'H' in buffer");
}

#[test]
fn test_nested_layouts() {
    let mut terminal = create_test_terminal(80, 24);

    terminal
        .draw(|frame| {
            let outer = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0)])
                .split(frame.area());

            let inner = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(34), Constraint::Min(10)])
                .split(outer[1]);

            // Verify nested layout dimensions
            assert_eq!(inner[0].width, 34);
            assert!(inner[1].width >= 10);
        })
        .unwrap();
}

#[test]
fn test_style_application() {
    let mut terminal = create_test_terminal(20, 3);

    terminal
        .draw(|frame| {
            let block = Block::default()
                .title("Styled")
                .border_style(Style::default().fg(Color::Red))
                .borders(Borders::ALL);

            frame.render_widget(block, frame.area());
        })
        .unwrap();

    // Test passes if no panic - style was applied
}

#[test]
fn test_multiple_widgets() {
    let mut terminal = create_test_terminal(80, 24);

    terminal
        .draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Min(0),
                ])
                .split(frame.area());

            let block1 = Block::default().title("Block 1").borders(Borders::ALL);
            let block2 = Block::default().title("Block 2").borders(Borders::ALL);
            let block3 = Block::default().title("Block 3").borders(Borders::ALL);

            frame.render_widget(block1, chunks[0]);
            frame.render_widget(block2, chunks[1]);
            frame.render_widget(block3, chunks[2]);
        })
        .unwrap();
}

#[test]
fn test_constraint_length() {
    let mut terminal = create_test_terminal(80, 24);

    terminal
        .draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(5),
                    Constraint::Length(10),
                    Constraint::Min(0),
                ])
                .split(frame.area());

            assert_eq!(chunks[0].height, 5);
            assert_eq!(chunks[1].height, 10);
            assert_eq!(chunks[2].height, 9); // 24 - 5 - 10 = 9
        })
        .unwrap();
}

#[test]
fn test_constraint_min() {
    let mut terminal = create_test_terminal(80, 24);

    terminal
        .draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(4), Constraint::Min(4)])
                .split(frame.area());

            // Both should get at least 4
            assert!(chunks[0].height >= 4);
            assert!(chunks[1].height >= 4);
            // Together they should fill the space
            assert_eq!(chunks[0].height + chunks[1].height, 24);
        })
        .unwrap();
}

#[test]
fn test_constraint_percentage() {
    let mut terminal = create_test_terminal(100, 20);

    terminal
        .draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
                .split(frame.area());

            assert_eq!(chunks[0].width, 30);
            assert_eq!(chunks[1].width, 70);
        })
        .unwrap();
}
