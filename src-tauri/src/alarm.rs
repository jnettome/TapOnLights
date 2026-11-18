//! The ringing notification: on Windows, an `alarm` toast that stays on screen and
//! loops the alarm sound until it's answered, with Snooze and Dismiss buttons that
//! act on the timer. The notification plugin can't loop a sound or set a scenario,
//! so this talks to WinRT directly.
//!
//! Each toast is tagged with its timer id, so dismissing or snoozing the timer
//! anywhere else (popup, web page, API) removes the toast and stops the sound.

use tauri::AppHandle;

use crate::timers::Timer;

/// Snooze length from the toast's button, in minutes.
pub const SNOOZE_MINUTES: f64 = 5.0;

/// Escape text for an XML element or attribute.
fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// Toast XML: alarm scenario (stays up, alarm audio), looping, two buttons.
fn toast_xml(title: &str, body: &str) -> String {
    format!(
        r#"<toast scenario="alarm" launch="open"><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual><audio src="ms-winsoundevent:Notification.Looping.Alarm" loop="true"/><actions><action content="Snooze {} min" arguments="snooze" activationType="foreground"/><action content="Dismiss" arguments="dismiss" activationType="foreground"/></actions></toast>"#,
        xml_escape(title),
        xml_escape(body),
        SNOOZE_MINUTES
    )
}

#[cfg(windows)]
mod imp {
    use std::path::Path;

    use tauri::AppHandle;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::TypedEventHandler;
    use windows::UI::Notifications::{
        ToastActivatedEventArgs, ToastNotification, ToastNotificationManager,
    };
    use windows::core::{HSTRING, IInspectable, Interface as _, Ref};

    use super::{SNOOZE_MINUTES, toast_xml};
    use crate::core;
    use crate::timers::Timer;

    const GROUP: &str = "timers";
    /// What Windows shows for builds run from target\ (no Start-menu shortcut
    /// registers our app id there); same rule as the notification plugin.
    const POWERSHELL_APP_ID: &str =
        r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe";

    fn app_id(app: &AppHandle) -> String {
        let dev_build = std::env::current_exe().ok().is_some_and(|exe| {
            exe.parent().is_some_and(|dir| {
                dir.ends_with(Path::new("target").join("release"))
                    || dir.ends_with(Path::new("target").join("debug"))
            })
        });
        if dev_build {
            POWERSHELL_APP_ID.into()
        } else {
            app.config().identifier.clone()
        }
    }

    pub fn show(
        app: &AppHandle,
        timer: &Timer,
        title: &str,
        body: &str,
        open: fn(&AppHandle),
    ) -> windows::core::Result<()> {
        let xml = XmlDocument::new()?;
        xml.LoadXml(&HSTRING::from(toast_xml(title, body)))?;
        let toast = ToastNotification::CreateToastNotification(&xml)?;
        toast.SetTag(&HSTRING::from(timer.id.to_string()))?;
        toast.SetGroup(&HSTRING::from(GROUP))?;

        let handle = app.clone();
        let id = timer.id;
        toast.Activated(&TypedEventHandler::new(
            move |_: Ref<ToastNotification>, args: Ref<IInspectable>| {
                let action = args
                    .as_ref()
                    .and_then(|a| a.cast::<ToastActivatedEventArgs>().ok())
                    .and_then(|a| a.Arguments().ok())
                    .map(|a| a.to_string())
                    .unwrap_or_default();
                match action.as_str() {
                    "snooze" => {
                        let _ = core::extend_timer(&handle, id, SNOOZE_MINUTES);
                    }
                    "dismiss" => {
                        let _ = core::cancel_timer(&handle, id);
                    }
                    _ => open(&handle),
                }
                Ok(())
            },
        ))?;

        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id(app)))?
            .Show(&toast)
    }

    pub fn hide(app: &AppHandle, id: u64) {
        if let Ok(history) = ToastNotificationManager::History() {
            let _ = history.RemoveGroupedTagWithId(
                &HSTRING::from(id.to_string()),
                &HSTRING::from(GROUP),
                &HSTRING::from(app_id(app)),
            );
        }
    }
}

/// Show the ringing toast for `timer`. Clicking the toast itself calls `open`
/// (to bring up the popup). Returns false if it couldn't be shown.
pub fn show(app: &AppHandle, timer: &Timer, title: &str, body: &str, open: fn(&AppHandle)) -> bool {
    #[cfg(windows)]
    {
        match imp::show(app, timer, title, body, open) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("Could not show the alarm notification: {e}");
                false
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (app, timer, title, body, open);
        false
    }
}

/// Remove timer `id`'s ringing toast, if it's still up (stops its sound).
pub fn hide(app: &AppHandle, id: u64) {
    #[cfg(windows)]
    imp::hide(app, id);
    #[cfg(not(windows))]
    let _ = (app, id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_text_is_escaped() {
        let xml = toast_xml("Pasta & <sauce>", "It's \"done\"");
        assert!(xml.contains("<text>Pasta &amp; &lt;sauce&gt;</text>"));
        assert!(xml.contains("<text>It&apos;s &quot;done&quot;</text>"));
        assert!(xml.contains(r#"loop="true""#) && xml.contains(r#"scenario="alarm""#));
        assert!(xml.contains("Snooze 5 min"));
    }
}
