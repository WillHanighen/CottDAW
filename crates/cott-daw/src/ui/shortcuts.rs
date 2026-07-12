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
}

pub fn handle(app: &mut CottApp, ctx: &Context) {
    // Let focused text fields and other keyboard-driven widgets consume input.
    if ctx.wants_keyboard_input() {
        return;
    }

    let action = ctx.input_mut(|input| {
        let command_shift = Modifiers::COMMAND | Modifiers::SHIFT;

        if consume_initial_key(input, command_shift, Key::Z)
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
        Some(ShortcutAction::Undo) => app.undo(),
        Some(ShortcutAction::Redo) => app.redo(),
        Some(ShortcutAction::Save) => app.save_project(),
        Some(ShortcutAction::Open) => app.load_project(),
        Some(ShortcutAction::Export) => app.export_mix(),
        Some(ShortcutAction::DeleteSelection) => {
            if app.ui.lower_tab == LowerTab::Graph {
                app.remove_selected_graph_node();
            } else if app.ui.selected_node.is_some() && app.ui.lower_tab == LowerTab::Plugins {
                app.remove_selected_graph_node();
            } else if app.ui.selected_clip.is_some() {
                app.remove_selected_clip();
            } else {
                app.remove_selected_graph_node();
            }
        }
        None => {}
    }
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
