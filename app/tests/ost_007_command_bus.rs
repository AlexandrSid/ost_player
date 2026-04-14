use ost_player::command_bus::{BusMessage, CommandBus, CommandSource};
use ost_player::tui::action::Action;

fn collect_all(bus: &CommandBus) -> Vec<BusMessage> {
    let mut out = Vec::new();
    while let Some(msg) = bus.try_recv() {
        out.push(msg);
    }
    out
}

#[test]
fn commands_from_multiple_sources_are_delivered_in_order_and_tagged() {
    let bus = CommandBus::new();

    bus.emit_action(
        CommandSource::Tui,
        Action::SetStatus("from_tui_1".to_string()),
    );
    bus.emit_action(
        CommandSource::Hotkey,
        Action::SetStatus("from_hotkey_2".to_string()),
    );
    bus.emit_action(
        CommandSource::System,
        Action::SetStatus("from_system_3".to_string()),
    );

    let msgs = collect_all(&bus);
    assert_eq!(msgs.len(), 3);

    match &msgs[0] {
        BusMessage::Command(cmd) => {
            assert_eq!(cmd.source, CommandSource::Tui);
            match &cmd.action {
                Action::SetStatus(s) => assert_eq!(s, "from_tui_1"),
                _ => panic!("expected SetStatus action"),
            }
        }
    }

    match &msgs[1] {
        BusMessage::Command(cmd) => {
            assert_eq!(cmd.source, CommandSource::Hotkey);
            match &cmd.action {
                Action::SetStatus(s) => assert_eq!(s, "from_hotkey_2"),
                _ => panic!("expected SetStatus action"),
            }
        }
    }

    match &msgs[2] {
        BusMessage::Command(cmd) => {
            assert_eq!(cmd.source, CommandSource::System);
            match &cmd.action {
                Action::SetStatus(s) => assert_eq!(s, "from_system_3"),
                _ => panic!("expected SetStatus action"),
            }
        }
    }
}

#[derive(Default)]
struct MockTuiApp {
    applied: Vec<Action>,
}

impl MockTuiApp {
    fn apply(&mut self, action: Action) {
        self.applied.push(action);
    }
}

fn drain_and_apply_until_quit(app: &mut MockTuiApp, bus: &CommandBus) -> bool {
    while let Some(msg) = bus.try_recv() {
        match msg {
            BusMessage::Command(cmd) => {
                let quit = matches!(cmd.action, Action::Quit);
                app.apply(cmd.action);
                if quit {
                    return true;
                }
            }
        }
    }
    false
}

#[test]
fn terminal_like_drain_applies_actions_in_order_and_stops_on_quit() {
    let bus = CommandBus::new();

    bus.emit_action(
        CommandSource::Tui,
        Action::SetStatus("before_quit".to_string()),
    );
    bus.emit_action(CommandSource::Hotkey, Action::Quit);
    bus.emit_action(
        CommandSource::System,
        Action::SetStatus("after_quit_should_not_apply".to_string()),
    );

    let mut app = MockTuiApp::default();
    let quit = drain_and_apply_until_quit(&mut app, &bus);
    assert!(quit, "quit should be observed by drain loop");

    assert_eq!(app.applied.len(), 2, "actions after Quit must not be applied");
    match &app.applied[0] {
        Action::SetStatus(s) => assert_eq!(s, "before_quit"),
        _ => panic!("expected first applied action to be SetStatus"),
    }
    assert!(matches!(app.applied[1], Action::Quit));
}

