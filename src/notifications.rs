use std::collections::HashMap;
use zbus::{dbus_proxy, zvariant::Value};
#[dbus_proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
pub trait Notifications {
    fn get_capabilities(&self) -> zbus::Result<(Vec<String>,)>;
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: &HashMap<&str, Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;
    fn close_notification(&self, id: u32) -> zbus::Result<()>;
    fn get_server_information(&self) -> zbus::Result<(String, String, String, String)>;
    #[dbus_proxy(signal)]
    fn notification_closed(&self, id: u32, reason: u32) -> Result<()>;
    #[dbus_proxy(signal)]
    fn action_invoked(&self, id: u32, action_key: String) -> Result<()>;
}

#[repr(u8)]
pub enum Urgency {
    Low = 0,
    Normal = 1,
    Critical = 2,
}

pub const MAX_SIZE: usize = 1usize << 21; // This is 2MiB, more than enough
pub const MAX_WIDTH: i32 = 255;
pub const MAX_HEIGHT: i32 = 255;

fn serialize_image(
    untrusted_width: i32,
    untrusted_height: i32,
    untrusted_rowstride: i32,
    untrusted_has_alpha: bool,
    untrusted_bits_per_sample: i32,
    untrusted_channels: i32,
    untrusted_data: &[u8],
) -> Result<Value, &'static str> {
    // sanitize start
    let has_alpha = untrusted_has_alpha; // no sanitization required
    if untrusted_width < 1 || untrusted_height < 1 || untrusted_rowstride < 3 {
        return Err("Too small width, height, or stride");
    }

    if untrusted_data.len() > MAX_SIZE {
        return Err("Too much data");
    }

    if untrusted_bits_per_sample != 8 {
        return Err("Wrong number of bits per sample");
    }

    let bits_per_sample = untrusted_bits_per_sample;
    let data = untrusted_data;
    let channels = 3i32 + untrusted_has_alpha as i32;

    if untrusted_channels != channels {
        return Err("Wrong number of channels");
    }

    if untrusted_width > MAX_WIDTH || untrusted_height > MAX_HEIGHT {
        return Err("Width or height too large");
    }

    if untrusted_data.len() as i32 / untrusted_height < untrusted_rowstride {
        return Err("Image too large");
    }

    if untrusted_rowstride / channels < untrusted_width {
        return Err("Row stride too small");
    }

    let height = untrusted_height;
    let width = untrusted_width;
    let rowstride = untrusted_rowstride;
    // sanitize end

    return Ok(Value::from((
        width,
        height,
        rowstride,
        has_alpha,
        bits_per_sample,
        channels,
        data,
    )));
}

#[repr(transparent)]
pub struct TrustedStr(String);

impl TrustedStr {
    pub fn new(arg: String) -> Self {
        // FIXME: validate this.  The current C API is unsuitable as it only returns
        // a boolean rather than replacing forbidden characters or even indicating
        // what those forbidden characters are.  This should be fixed on the C side
        // rather than by ugly hacks (such as character-by-character loops).
        return TrustedStr(arg);
    }

    pub fn inner(&self) -> &String {
        &self.0
    }
}

async fn send_notification(
    connection: &NotificationsProxy<'_>,
    _suppress_sound: bool,
    _transient: bool,
    urgency: Option<Urgency>,
    // This is just an ID, and it can't be validated in a non-racy way anyway.
    // I assume that any decent notification daemon will handle an invalid ID
    // value correctly, but this code should probably test for this at the start
    // so that it cannot be used with a server that crashes in this case.
    replaces: u32,
    summary: TrustedStr,
    body: TrustedStr,
    actions: Vec<TrustedStr>,
    _category: Option<TrustedStr>,
    expire_timeout: i32,
) -> zbus::Result<u32> {
    if expire_timeout < -1 {
        return Err(zbus::Error::Unsupported);
    }

    // In the future this should be a validated application name prefixed
    // by the qube name.
    let application_name = "";

    // Ideally the icon would be associated with the calling application,
    // with an image suitably processed by Qubes OS to indicate trust.
    // However, there is no good way to do that in practice, so just pass
    // an empty string to indicate "no icon".
    let icon = "";

    // this is slow but I don't care, the dbus call is orders of magnitude slower
    let actions: Vec<&str> = actions.iter().map(|x| &*x.0).collect();

    let mut hints = HashMap::new();
    if let Some(urgency) = urgency {
        let urgency = match urgency {
            Urgency::Low => &0,
            Urgency::Normal => &1,
            Urgency::Critical => &2,
        };
        hints.insert(
            "urgency",
            <zbus::zvariant::Value<'_> as From<&'_ u8>>::from(urgency),
        );
    }
    connection
        .notify(
            application_name,
            replaces,
            icon,
            &*summary.0,
            &*body.0,
            &*actions,
            &hints,
            expire_timeout,
        )
        .await
}
