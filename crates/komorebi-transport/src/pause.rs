use komorebi_protocol::message::SocketMessage;

/// Whether a daemon that is paused still handles `message`
///
/// Only `TogglePause` (to be able to resume), `State`, `GlobalState` and `Stop` are. This is
/// daemon policy and not part of the wire format, which is why it is a function here and not a
/// method of [`SocketMessage`].
#[must_use]
pub fn allowed_while_paused(message: &SocketMessage) -> bool {
    matches!(
        message,
        SocketMessage::TogglePause
            | SocketMessage::State
            | SocketMessage::GlobalState
            | SocketMessage::Stop
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_four_messages_are_allowed_while_paused() {
        for message in [
            SocketMessage::TogglePause,
            SocketMessage::State,
            SocketMessage::GlobalState,
            SocketMessage::Stop,
        ] {
            assert!(allowed_while_paused(&message), "{message}");
        }

        for message in [
            SocketMessage::Retile,
            SocketMessage::UnmanageFocusedWindow,
            SocketMessage::ReloadConfiguration,
        ] {
            assert!(!allowed_while_paused(&message), "{message}");
        }
    }
}
