use std::collections::HashMap;

use secret_service::{EncryptionType, SecretService};

use crate::error::Result;

const ATTR_SERVICE: &str = "service";
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
        Ok(Some(String::from_utf8_lossy(&secret).to_string()))
    }

    async fn set_secret(
        &self,
        label: &str,
        service_name: &str,
        account_name: &str,
        secret_value: &str,
    ) -> Result<()> {
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
        Ok(())
    }

    fn attributes<'a>(service_name: &'a str, account_name: &'a str) -> HashMap<&'a str, &'a str> {
        HashMap::from([(ATTR_SERVICE, service_name), (ATTR_ACCOUNT, account_name)])
    }
}
