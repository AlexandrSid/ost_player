use crate::tui::action::Action;
use std::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSource {
    Tui,
    Hotkey,
    System,
}

#[derive(Debug, Clone)]
pub struct CommandEnvelope {
    pub action: Action,
    pub source: CommandSource,
}

#[derive(Debug, Clone)]
pub enum BusMessage {
    Command(CommandEnvelope),
}

#[derive(Debug)]
pub struct CommandBus {
    rx: mpsc::Receiver<BusMessage>,
    tx: mpsc::Sender<BusMessage>,
}

impl CommandBus {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<BusMessage>();
        Self { rx, tx }
    }

    pub fn sender(&self) -> mpsc::Sender<BusMessage> {
        self.tx.clone()
    }

    pub fn try_recv(&self) -> Option<BusMessage> {
        self.rx.try_recv().ok()
    }

    pub fn emit_action(&self, source: CommandSource, action: Action) {
        let _ = self.tx.send(BusMessage::Command(CommandEnvelope { action, source }));
    }
}

