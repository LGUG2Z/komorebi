#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::border_manager;
use crate::stackbar_manager;
use crate::stackbar_manager::STACKBAR_FOCUSED_TEXT_COLOUR;
use crate::stackbar_manager::STACKBAR_TAB_BACKGROUND_COLOUR;
use crate::stackbar_manager::STACKBAR_UNFOCUSED_TEXT_COLOUR;
use crate::KomorebiTheme;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use crossbeam_utils::atomic::AtomicCell;
use komorebi_themes::colour::Colour;
use komorebi_themes::Base16Wrapper;
use std::ops::Deref;
use std::sync::atomic::Ordering;
use std::sync::OnceLock;

pub struct Notification(KomorebiTheme);

pub static CURRENT_THEME: AtomicCell<Option<KomorebiTheme>> = AtomicCell::new(None);

impl Deref for Notification {
    type Target = KomorebiTheme;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

static CHANNEL: OnceLock<(Sender<Notification>, Receiver<Notification>)> = OnceLock::new();

pub fn channel() -> &'static (Sender<Notification>, Receiver<Notification>) {
    CHANNEL.get_or_init(|| crossbeam_channel::bounded(20))
}

fn event_tx() -> Sender<Notification> {
    channel().0.clone()
}

fn event_rx() -> Receiver<Notification> {
    channel().1.clone()
}

// Currently this should only be used for async focus updates, such as
// when an animation finishes and we need to focus to set the cursor
// position if the user has mouse follows focus enabled
pub fn send_notification(theme: KomorebiTheme) {
    if event_tx().try_send(Notification(theme)).is_err() {
        tracing::warn!("channel is full; dropping notification")
    }
}

pub fn listen_for_notifications() {
    std::thread::spawn(move || loop {
        match handle_notifications() {
            Ok(()) => {
                tracing::warn!("restarting finished thread");
            }
            Err(error) => {
                tracing::warn!("restarting failed thread: {}", error);
            }
        }
    });
}

pub fn handle_notifications() -> color_eyre::Result<()> {
    tracing::info!("listening");

    let receiver = event_rx();

    for notification in receiver {
        let theme = &notification.0;

        let (
            single_border,
            stack_border,
            monocle_border,
            floating_border,
            unfocused_border,
            unfocused_locked_border,
            stackbar_focused_text,
            stackbar_unfocused_text,
            stackbar_background,
        ) = match theme {
            KomorebiTheme::Catppuccin {
                name,
                single_border,
                stack_border,
                monocle_border,
                floating_border,
                unfocused_border,
                unfocused_locked_border,
                stackbar_focused_text,
                stackbar_unfocused_text,
                stackbar_background,
                ..
            } => {
                let single_border = single_border
                    .unwrap_or(komorebi_themes::CatppuccinValue::Blue)
                    .color32(name.as_theme());

                let stack_border = stack_border
                    .unwrap_or(komorebi_themes::CatppuccinValue::Green)
                    .color32(name.as_theme());

                let monocle_border = monocle_border
                    .unwrap_or(komorebi_themes::CatppuccinValue::Pink)
                    .color32(name.as_theme());

                let floating_border = floating_border
                    .unwrap_or(komorebi_themes::CatppuccinValue::Yellow)
                    .color32(name.as_theme());

                let unfocused_border = unfocused_border
                    .unwrap_or(komorebi_themes::CatppuccinValue::Base)
                    .color32(name.as_theme());

                let unfocused_locked_border = unfocused_locked_border
                    .unwrap_or(komorebi_themes::CatppuccinValue::Red)
                    .color32(name.as_theme());

                let stackbar_focused_text = stackbar_focused_text
                    .unwrap_or(komorebi_themes::CatppuccinValue::Green)
                    .color32(name.as_theme());

                let stackbar_unfocused_text = stackbar_unfocused_text
                    .unwrap_or(komorebi_themes::CatppuccinValue::Text)
                    .color32(name.as_theme());

                let stackbar_background = stackbar_background
                    .unwrap_or(komorebi_themes::CatppuccinValue::Base)
                    .color32(name.as_theme());

                (
                    single_border,
                    stack_border,
                    monocle_border,
                    floating_border,
                    unfocused_border,
                    unfocused_locked_border,
                    stackbar_focused_text,
                    stackbar_unfocused_text,
                    stackbar_background,
                )
            }
            KomorebiTheme::Base16 {
                name,
                single_border,
                stack_border,
                monocle_border,
                floating_border,
                unfocused_border,
                unfocused_locked_border,
                stackbar_focused_text,
                stackbar_unfocused_text,
                stackbar_background,
                ..
            } => {
                let single_border = single_border
                    .unwrap_or(komorebi_themes::Base16Value::Base0D)
                    .color32(Base16Wrapper::Base16(*name));

                let stack_border = stack_border
                    .unwrap_or(komorebi_themes::Base16Value::Base0B)
                    .color32(Base16Wrapper::Base16(*name));

                let monocle_border = monocle_border
                    .unwrap_or(komorebi_themes::Base16Value::Base0F)
                    .color32(Base16Wrapper::Base16(*name));

                let unfocused_border = unfocused_border
                    .unwrap_or(komorebi_themes::Base16Value::Base01)
                    .color32(Base16Wrapper::Base16(*name));

                let unfocused_locked_border = unfocused_locked_border
                    .unwrap_or(komorebi_themes::Base16Value::Base08)
                    .color32(Base16Wrapper::Base16(*name));

                let floating_border = floating_border
                    .unwrap_or(komorebi_themes::Base16Value::Base09)
                    .color32(Base16Wrapper::Base16(*name));

                let stackbar_focused_text = stackbar_focused_text
                    .unwrap_or(komorebi_themes::Base16Value::Base0B)
                    .color32(Base16Wrapper::Base16(*name));

                let stackbar_unfocused_text = stackbar_unfocused_text
                    .unwrap_or(komorebi_themes::Base16Value::Base05)
                    .color32(Base16Wrapper::Base16(*name));

                let stackbar_background = stackbar_background
                    .unwrap_or(komorebi_themes::Base16Value::Base01)
                    .color32(Base16Wrapper::Base16(*name));

                (
                    single_border,
                    stack_border,
                    monocle_border,
                    floating_border,
                    unfocused_border,
                    unfocused_locked_border,
                    stackbar_focused_text,
                    stackbar_unfocused_text,
                    stackbar_background,
                )
            }
            KomorebiTheme::Custom {
                colours,
                single_border,
                stack_border,
                monocle_border,
                floating_border,
                unfocused_border,
                unfocused_locked_border,
                stackbar_focused_text,
                stackbar_unfocused_text,
                stackbar_background,
                ..
            } => {
                let single_border = single_border
                    .unwrap_or(komorebi_themes::Base16Value::Base0D)
                    .color32(Base16Wrapper::Custom(colours.clone()));

                let stack_border = stack_border
                    .unwrap_or(komorebi_themes::Base16Value::Base0B)
                    .color32(Base16Wrapper::Custom(colours.clone()));

                let monocle_border = monocle_border
                    .unwrap_or(komorebi_themes::Base16Value::Base0F)
                    .color32(Base16Wrapper::Custom(colours.clone()));

                let unfocused_border = unfocused_border
                    .unwrap_or(komorebi_themes::Base16Value::Base01)
                    .color32(Base16Wrapper::Custom(colours.clone()));

                let unfocused_locked_border = unfocused_locked_border
                    .unwrap_or(komorebi_themes::Base16Value::Base08)
                    .color32(Base16Wrapper::Custom(colours.clone()));

                let floating_border = floating_border
                    .unwrap_or(komorebi_themes::Base16Value::Base09)
                    .color32(Base16Wrapper::Custom(colours.clone()));

                let stackbar_focused_text = stackbar_focused_text
                    .unwrap_or(komorebi_themes::Base16Value::Base0B)
                    .color32(Base16Wrapper::Custom(colours.clone()));

                let stackbar_unfocused_text = stackbar_unfocused_text
                    .unwrap_or(komorebi_themes::Base16Value::Base05)
                    .color32(Base16Wrapper::Custom(colours.clone()));

                let stackbar_background = stackbar_background
                    .unwrap_or(komorebi_themes::Base16Value::Base01)
                    .color32(Base16Wrapper::Custom(colours.clone()));

                (
                    single_border,
                    stack_border,
                    monocle_border,
                    floating_border,
                    unfocused_border,
                    unfocused_locked_border,
                    stackbar_focused_text,
                    stackbar_unfocused_text,
                    stackbar_background,
                )
            }
        };

        border_manager::FOCUSED.store(u32::from(Colour::from(single_border)), Ordering::SeqCst);
        border_manager::MONOCLE.store(u32::from(Colour::from(monocle_border)), Ordering::SeqCst);
        border_manager::STACK.store(u32::from(Colour::from(stack_border)), Ordering::SeqCst);
        border_manager::FLOATING.store(u32::from(Colour::from(floating_border)), Ordering::SeqCst);
        border_manager::UNFOCUSED
            .store(u32::from(Colour::from(unfocused_border)), Ordering::SeqCst);
        border_manager::UNFOCUSED_LOCKED.store(
            u32::from(Colour::from(unfocused_locked_border)),
            Ordering::SeqCst,
        );

        STACKBAR_TAB_BACKGROUND_COLOUR.store(
            u32::from(Colour::from(stackbar_background)),
            Ordering::SeqCst,
        );

        STACKBAR_FOCUSED_TEXT_COLOUR.store(
            u32::from(Colour::from(stackbar_focused_text)),
            Ordering::SeqCst,
        );

        STACKBAR_UNFOCUSED_TEXT_COLOUR.store(
            u32::from(Colour::from(stackbar_unfocused_text)),
            Ordering::SeqCst,
        );

        CURRENT_THEME.store(Some(notification.0));

        border_manager::send_force_update();
        stackbar_manager::send_notification();
    }

    Ok(())
}
