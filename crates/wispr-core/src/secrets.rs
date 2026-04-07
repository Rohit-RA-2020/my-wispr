#[cfg(target_os = "linux")]
use secret_service::{EncryptionType, SecretService};
#[cfg(target_os = "linux")]
use std::collections::HashMap;
#[cfg(target_os = "macos")]
use std::process::Command;

use crate::error::Result;

#[cfg(target_os = "linux")]
const ATTR_SERVICE: &str = "service";
#[cfg(target_os = "linux")]
const ATTR_ACCOUNT: &str = "account";

const DEEPGRAM_SECRET_LABEL: &str = "Wispr Deepgram API Key";
const DEEPGRAM_SERVICE: &str = "io.wispr.deepgram";

const LLM_SECRET_LABEL: &str = "Wispr Intelligence API Key";
const LLM_SERVICE: &str = "io.wispr.intelligence";

pub struct SecretStore;

impl SecretStore {
    pub async fn connect() -> Result<Self> {
        Ok(Self)
    }

    pub async fn get_api_key(&self) -> Result<Option<String>> {
        self.get_secret(DEEPGRAM_SERVICE, "default").await
    }

    pub async fn set_api_key(&self, api_key: &str) -> Result<()> {
        self.set_secret(DEEPGRAM_SECRET_LABEL, DEEPGRAM_SERVICE, "default", api_key)
            .await
    }

    pub async fn get_llm_api_key(&self) -> Result<Option<String>> {
        self.get_secret(LLM_SERVICE, "default").await
    }

    pub async fn set_llm_api_key(&self, api_key: &str) -> Result<()> {
        self.set_secret(LLM_SECRET_LABEL, LLM_SERVICE, "default", api_key)
            .await
    }

    async fn get_secret(&self, service_name: &str, account_name: &str) -> Result<Option<String>> {
        #[cfg(target_os = "linux")]
        {
            let service = SecretService::connect(EncryptionType::Plain).await?;
            let search = service
                .search_items(Self::attributes(service_name, account_name))
                .await?;
            let item = if let Some(item) = search.unlocked.first() {
                item
            } else if let Some(item) = search.locked.first() {
                item.unlock().await?;
                item
            } else {
                return Ok(None);
            };

            let secret = item.get_secret().await?;
            return Ok(Some(String::from_utf8_lossy(&secret).to_string()));
        }

        #[cfg(target_os = "macos")]
        {
            return keychain_get(service_name, account_name);
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (service_name, account_name);
            Ok(None)
        }
    }

    async fn set_secret(
        &self,
        label: &str,
        service_name: &str,
        account_name: &str,
        secret_value: &str,
    ) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            let service = SecretService::connect(EncryptionType::Plain).await?;
            let collection = service.get_default_collection().await?;
            collection
                .create_item(
                    label,
                    Self::attributes(service_name, account_name),
                    secret_value.as_bytes(),
                    true,
                    "text/plain",
                )
                .await?;
            return Ok(());
        }

        #[cfg(target_os = "macos")]
        {
            let _ = label;
            return keychain_set(service_name, account_name, secret_value);
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (label, service_name, account_name, secret_value);
            Ok(())
        }
    }

    #[cfg(target_os = "linux")]
    fn attributes<'a>(service_name: &'a str, account_name: &'a str) -> HashMap<&'a str, &'a str> {
        HashMap::from([(ATTR_SERVICE, service_name), (ATTR_ACCOUNT, account_name)])
    }
}

#[cfg(target_os = "macos")]
fn keychain_get(service_name: &str, account_name: &str) -> Result<Option<String>> {
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            service_name,
            "-a",
            account_name,
            "-w",
        ])
        .output()?;

    if output.status.success() {
        return Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ));
    }

    Ok(None)
}

#[cfg(target_os = "macos")]
fn keychain_set(service_name: &str, account_name: &str, secret_value: &str) -> Result<()> {
    let output = Command::new("security")
        .args([
            "add-generic-password",
            "-U",
            "-s",
            service_name,
            "-a",
            account_name,
            "-w",
            secret_value,
        ])
        .output()?;

    if output.status.success() {
        Ok(())
    } else {
        Err(crate::WisprError::InvalidState(format!(
            "failed to write keychain secret for {service_name}/{account_name}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}
