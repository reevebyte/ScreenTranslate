use screentrans_core::{AppError, ErrorCode, Result, ports::SecretStore};
use std::{collections::HashMap, sync::Mutex};

const SERVICE: &str = "io.github.reevebyte.screentranslate.v2";

#[derive(Default)]
pub struct SystemSecretStore;

impl SecretStore for SystemSecretStore {
    fn get(&self, name: &str) -> Result<Option<String>> {
        match keyring::Entry::new(SERVICE, name).and_then(|entry| entry.get_password()) {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(credentials_error()),
        }
    }
    fn set(&self, name: &str, secret: &str) -> Result<()> {
        keyring::Entry::new(SERVICE, name)
            .and_then(|entry| entry.set_password(secret))
            .map_err(|_| credentials_error())
    }
    fn delete(&self, name: &str) -> Result<()> {
        match keyring::Entry::new(SERVICE, name).and_then(|entry| entry.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(credentials_error()),
        }
    }
}

fn credentials_error() -> AppError {
    AppError::new(
        ErrorCode::Credentials,
        "系统凭据库不可用或已锁定；可以只在本次会话中使用密钥",
    )
    .action("providers")
}

pub struct CredentialVault {
    persistent: Box<dyn SecretStore>,
    session: Mutex<HashMap<String, String>>,
}

impl Default for CredentialVault {
    fn default() -> Self {
        Self {
            persistent: Box::new(SystemSecretStore),
            session: Mutex::new(HashMap::new()),
        }
    }
}

impl CredentialVault {
    pub fn session_only() -> Self {
        struct SessionOnly;
        impl SecretStore for SessionOnly {
            fn get(&self, _name: &str) -> Result<Option<String>> {
                Ok(None)
            }
            fn set(&self, _name: &str, _secret: &str) -> Result<()> {
                Err(credentials_error())
            }
            fn delete(&self, _name: &str) -> Result<()> {
                Ok(())
            }
        }
        Self {
            persistent: Box::new(SessionOnly),
            session: Mutex::new(HashMap::new()),
        }
    }
    pub fn get(&self, name: &str) -> Result<Option<String>> {
        if let Some(value) = self
            .session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
        {
            return Ok(Some(value.clone()));
        }
        self.persistent.get(name)
    }

    /// Returns whether the credential is durable. A failed keyring write never creates a plaintext file.
    pub fn set(&self, name: &str, secret: &str, persistent: bool) -> Result<bool> {
        if secret.is_empty() {
            return Err(AppError::new(ErrorCode::InvalidInput, "密钥不能为空"));
        }
        let saved = persistent && self.persistent.set(name, secret).is_ok();
        self.session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name.into(), secret.into());
        Ok(saved)
    }

    pub fn delete(&self, name: &str) -> Result<()> {
        self.persistent.delete(name)?;
        self.session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(name);
        Ok(())
    }
}

pub fn decode_legacy(value: &str) -> Result<String> {
    if !value.starts_with("dpapi:v1:") {
        return Ok(value.into());
    }
    #[cfg(windows)]
    {
        use base64::Engine;
        use windows::Win32::{
            Foundation::{HLOCAL, LocalFree},
            Security::Cryptography::{
                CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
            },
        };
        let mut bytes = base64::engine::general_purpose::STANDARD
            .decode(&value[9..])
            .map_err(|_| credentials_error())?;
        let input = CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_mut_ptr(),
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
            .map_err(|_| credentials_error())?;
            let decoded = String::from_utf8(
                std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec(),
            );
            let _ = LocalFree(HLOCAL(output.pbData as *mut _));
            decoded.map_err(|_| credentials_error())
        }
    }
    #[cfg(not(windows))]
    Err(AppError::new(
        ErrorCode::Credentials,
        "旧 DPAPI 密钥只能在原 Windows 账户中导入，请重新填写",
    ))
}
