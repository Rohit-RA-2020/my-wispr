use std::collections::HashMap;

use futures_util::StreamExt;
use tokio::sync::mpsc;
use uuid::Uuid;
use wispr_core::{
    error::{Result, WisprError},
    models::HotkeyBinding,
};
use zbus::{Connection, Proxy, zvariant::OwnedObjectPath};
use zvariant::{OwnedValue, Str, Value};

const PORTAL_SERVICE: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const PORTAL_IFACE: &str = "org.freedesktop.portal.GlobalShortcuts";
const REQUEST_IFACE: &str = "org.freedesktop.portal.Request";

pub async fn register_toggle_shortcut(
    connection: &Connection,
    hotkey: &HotkeyBinding,
) -> Result<mpsc::Receiver<()>> {
    let proxy = Proxy::new(connection, PORTAL_SERVICE, PORTAL_PATH, PORTAL_IFACE).await?;

    let session_handle_token = format!("wispr-session-{}", Uuid::new_v4().simple());
    let create_handle_token = format!("wispr-request-{}", Uuid::new_v4().simple());
    let create_options = HashMap::from([
        ("handle_token", owned_string(&create_handle_token)?),
        ("session_handle_token", owned_string(&session_handle_token)?),
    ]);
    let request_handle: OwnedObjectPath = proxy.call("CreateSession", &(create_options)).await?;
    let create_results = wait_for_request_response(connection, request_handle).await?;
    let session_handle = create_results
        .get("session_handle")
        .and_then(string_from_value)
        .ok_or_else(|| {
            WisprError::InvalidState("portal did not return a session_handle".to_string())
        })?;

    let bind_handle_token = format!("wispr-bind-{}", Uuid::new_v4().simple());
    let bind_options = HashMap::from([("handle_token", owned_string(&bind_handle_token)?)]);
    let shortcut_properties = HashMap::from([
        (
            "description".to_string(),
            owned_string(&hotkey.description)?,
        ),
        (
            "preferred_trigger".to_string(),
            owned_string(&hotkey.preferred_trigger)?,
        ),
    ]);
    let shortcuts = vec![(hotkey.id.clone(), shortcut_properties)];
    let bind_request: OwnedObjectPath = proxy
        .call(
            "BindShortcuts",
            &(
                OwnedObjectPath::try_from(session_handle.clone())
                    .map_err(|err| WisprError::Message(err.to_string()))?,
                shortcuts,
                String::new(),
                bind_options,
            ),
        )
        .await?;
    let _bind_results = wait_for_request_response(connection, bind_request).await?;

    let (tx, rx) = mpsc::channel(8);
    let session_path = OwnedObjectPath::try_from(session_handle)
        .map_err(|err| WisprError::Message(err.to_string()))?;
    let signal_proxy = Proxy::new(connection, PORTAL_SERVICE, PORTAL_PATH, PORTAL_IFACE).await?;
    let mut stream = signal_proxy.receive_signal("Activated").await?;
    let hotkey_id = hotkey.id.clone();

    tokio::spawn(async move {
        while let Some(signal) = stream.next().await {
            let Ok(args) = signal
                .body()
                .deserialize::<(OwnedObjectPath, String, u64, HashMap<String, OwnedValue>)>()
            else {
                continue;
            };
            if args.0 == session_path && args.1 == hotkey_id {
                let _ = tx.send(()).await;
            }
        }
    });

    Ok(rx)
}

async fn wait_for_request_response(
    connection: &Connection,
    path: OwnedObjectPath,
) -> Result<HashMap<String, OwnedValue>> {
    let proxy = Proxy::new(connection, PORTAL_SERVICE, path.as_str(), REQUEST_IFACE).await?;
    let mut signals = proxy.receive_signal("Response").await?;
    let Some(signal) = signals.next().await else {
        return Err(WisprError::InvalidState(
            "portal request finished without a Response signal".to_string(),
        ));
    };

    let (response, results): (u32, HashMap<String, OwnedValue>) = signal
        .body()
        .deserialize()
        .map_err(|err| WisprError::Message(err.to_string()))?;
    if response != 0 {
        return Err(WisprError::InvalidState(format!(
            "portal request failed with response code {response}"
        )));
    }
    Ok(results)
}

fn string_from_value(value: &OwnedValue) -> Option<String> {
    if let Ok(text) = <&str>::try_from(value) {
        return Some(text.to_string());
    }
    if let Ok(text) = <String>::try_from(value.clone()) {
        return Some(text);
    }
    None
}

fn owned_string(value: &str) -> Result<OwnedValue> {
    OwnedValue::try_from(Value::from(Str::from(value)))
        .map_err(|err| WisprError::Message(err.to_string()))
}
