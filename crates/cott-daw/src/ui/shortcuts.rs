use super::LowerTab;
use crate::app::CottApp;
use eframe::egui::{Context, Event, InputState, Key, Modifiers};

#[derive(Debug, Clone, Copy)]
enum ShortcutAction {
    TogglePlayStop,
    Stop,
    ToggleLoop,
    ToggleBrowser,
    Undo,
    Redo,
    Save,
    Open,
    Export,
    DeleteSelection,
    Copy,
    Paste,
    Duplicate,
    RenameTrack,
}

pub fn handle(app: &mut CottApp, ctx: &Context) {
    // Let focused text fields and other keyboard-driven widgets consume input.
    if ctx.wants_keyboard_input() {
        return;
    }

    let action = ctx.input_mut(|input| {
        let command_shift = Modifiers::COMMAND | Modifiers::SHIFT;

        // egui/winit turns Ctrl/Cmd+C/V into Copy/Paste events and often drops the
        // raw Key events. Paste only appears when the OS clipboard has text, so
        // copy handlers also seed a sentinel via Context::copy_text.
        if clipboard_event(input, ClipboardEventKind::Copy)
            || consume_initial_key(input, Modifiers::COMMAND, Key::C)
        {
            Some(ShortcutAction::Copy)
        } else if clipboard_event(input, ClipboardEventKind::Paste)
            || consume_initial_key(input, Modifiers::COMMAND, Key::V)
        {
            Some(ShortcutAction::Paste)
        } else if consume_initial_key(input, command_shift, Key::Z)
            || consume_initial_key(input, Modifiers::COMMAND, Key::Y)
        {
            Some(ShortcutAction::Redo)
        } else if consume_initial_key(input, Modifiers::COMMAND, Key::Z) {
            Some(ShortcutAction::Undo)
        } else if consume_initial_key(input, Modifiers::COMMAND, Key::S) {
            Some(ShortcutAction::Save)
        } else if consume_initial_key(input, Modifiers::COMMAND, Key::O) {
            Some(ShortcutAction::Open)
        } else if consume_initial_key(input, Modifiers::COMMAND, Key::E) {
            Some(ShortcutAction::Export)
        } else if consume_initial_key(input, Modifiers::COMMAND, Key::D) {
            Some(ShortcutAction::Duplicate)
        } else if consume_initial_key(input, Modifiers::NONE, Key::F2) {
            Some(ShortcutAction::RenameTrack)
        } else if consume_initial_key(input, Modifiers::NONE, Key::Space) {
            Some(ShortcutAction::TogglePlayStop)
        } else if consume_initial_key(input, Modifiers::NONE, Key::Home) {
            Some(ShortcutAction::Stop)
        } else if consume_initial_key(input, Modifiers::NONE, Key::L) {
            Some(ShortcutAction::ToggleLoop)
        } else if consume_initial_key(input, Modifiers::NONE, Key::B) {
            Some(ShortcutAction::ToggleBrowser)
        } else if consume_initial_key(input, Modifiers::NONE, Key::Delete)
            || consume_initial_key(input, Modifiers::NONE, Key::Backspace)
        {
            Some(ShortcutAction::DeleteSelection)
        } else {
            None
        }
    });

    match action {
        Some(ShortcutAction::TogglePlayStop) => app.toggle_play_stop(),
        Some(ShortcutAction::Stop) => app.stop(),
        Some(ShortcutAction::ToggleLoop) => app.toggle_loop(),
        Some(ShortcutAction::ToggleBrowser) => app.ui.show_browser = !app.ui.show_browser,
        Some(ShortcutAction::Undo) => {
            app.undo();
            app.prune_note_selection();
        }
        Some(ShortcutAction::Redo) => {
            app.redo();
            app.prune_note_selection();
        }
        Some(ShortcutAction::Save) => app.save_project(),
        Some(ShortcutAction::Open) => app.load_project(),
        Some(ShortcutAction::Export) => app.open_export_dialog(),
        Some(ShortcutAction::Copy) => {
            if app.ui.lower_tab == LowerTab::PianoRoll
                && !app.ui.selected_notes.is_empty()
                && app.ui.selected_notes_clip.is_some()
            {
                app.copy_selected_notes();
            } else if app.ui.selected_clip.is_some() {
                app.copy_selected_clip();
            }
        }
        Some(ShortcutAction::Paste) => {
            // Prefer the most recently filled clipboard. Note and clip copies clear
            // each other so this is unambiguous.
            if app.ui.note_clipboard.is_some() {
                app.paste_notes_at_hover();
            } else if app.ui.clip_clipboard.is_some() {
                app.paste_clip_at_hover();
            } else {
                app.status = "Nothing to paste".into();
            }
        }
        Some(ShortcutAction::Duplicate) => {
            if app.ui.selected_clip.is_some() {
                app.duplicate_selected_clip();
            }
        }
        Some(ShortcutAction::RenameTrack) => {
            if let Some(track_id) = app.ui.selected_track {
                if let Some(name) = app
                    .project
                    .tracks
                    .iter()
                    .find(|t| t.id == track_id)
                    .map(|t| t.name.clone())
                {
                    app.ui.renaming_track = Some((track_id, name));
                }
            }
        }
        Some(ShortcutAction::DeleteSelection) => {
            if app.ui.lower_tab == LowerTab::PianoRoll
                && !app.ui.selected_notes.is_empty()
                && app.ui.selected_notes_clip.is_some()
            {
                app.remove_selected_notes();
            } else if app.ui.lower_tab == LowerTab::Graph {
                app.remove_selected_graph_node();
            } else if app.ui.selected_node.is_some() && app.ui.lower_tab == LowerTab::Plugins {
                app.remove_selected_graph_node();
            } else if app.ui.selected_clip.is_some() {
                app.remove_selected_clip();
            } else if app.ui.selected_node.is_some() {
                app.remove_selected_graph_node();
            } else if app.ui.selected_track.is_some() {
                app.remove_selected_track();
            }
        }
        None => {}
    }
}

#[derive(Clone, Copy)]
enum ClipboardEventKind {
    Copy,
    Paste,
}

fn clipboard_event(input: &InputState, kind: ClipboardEventKind) -> bool {
    input.events.iter().any(|event| match kind {
        ClipboardEventKind::Copy => matches!(event, Event::Copy),
        ClipboardEventKind::Paste => matches!(event, Event::Paste(_)),
    })
}

fn consume_initial_key(input: &mut InputState, modifiers: Modifiers, key: Key) -> bool {
    let pressed = input.events.iter().any(|event| {
        matches!(
            event,
            Event::Key {
                key: event_key,
                pressed: true,
                repeat: false,
                modifiers: event_modifiers,
                ..
            } if *event_key == key && event_modifiers.matches_logically(modifiers)
        )
    });
    pressed && input.consume_key(modifiers, key)
}
