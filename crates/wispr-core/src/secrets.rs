use std::collections::HashMap;

use secret_service::{EncryptionType, SecretService};

use crate::error::Result;

const SECRET_LABEL: &str = "Wispr Deepgram API Key";
const ATTR_SERVICE: &str = "service";
const ATTR_ACCOUNT: &str = "account";
const ATTR_VALUE_SERVICE: &str = "io.wispr.deepgram";
const ATTR_VALUE_ACCOUNT: &str = "default";

pub struct SecretStore;

impl SecretStore {
    pub async fn connect() -> Result<Self> {
        Ok(Self)
    }

    pub async fn get_api_key(&self) -> Result<Option<String>> {
        let service = SecretService::connect(EncryptionType::Plain).await?;
        let search = service.search_items(Self::attributes()).await?;
        let item = if let Some(item) = search.unlocked.first() {
            item
        } else if let Some(item) = search.locked.first() {
            item.unlock().await?;
            item
        } else {
            return Ok(None);
        };

        let secret = item.get_secret().await?;
        Ok(Some(String::from_utf8_lossy(&secret).to_string()))
    }

    pub async fn set_api_key(&self, api_key: &str) -> Result<()> {
        let service = SecretService::connect(EncryptionType::Plain).await?;
        let collection = service.get_default_collection().await?;
        collection
            .create_item(
                SECRET_LABEL,
                Self::attributes(),
                api_key.as_bytes(),
                true,
                "text/plain",
            )
            .await?;
        Ok(())
    }

    fn attributes() -> HashMap<&'static str, &'static str> {
        HashMap::from([
            (ATTR_SERVICE, ATTR_VALUE_SERVICE),
            (ATTR_ACCOUNT, ATTR_VALUE_ACCOUNT),
        ])
    }
}
