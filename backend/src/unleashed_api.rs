//! The operations the HTTP handlers need, implemented on Unleashed guest
//! passes.

use std::{collections::HashSet, sync::OnceLock};

use chrono::Utc;
use chrono_tz::Tz;
use reqwest::StatusCode;
use tracing::{error, info, warn};

use crate::{
    environment::Environment,
    models::*,
    unleashed::{
        create::{CreateOutcome, CreateParams, build_form, parse_create_response},
        guest::{GuestPass, parse_guest_list},
        mapping::{
            current_rolling, has_live_rolling_for_ip, is_expired, is_rolling, rolling_name,
            to_voucher,
        },
        session::{Session, SessionError},
    },
};

pub static UNLEASHED_API: OnceLock<UnleashedAPI> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub url: String,
    pub username: String,
    pub password: String,
    pub ssid: String,
    pub verify_tls: bool,
    pub timezone: Tz,
    pub rolling_voucher_duration_minutes: u64,
}

impl ApiConfig {
    pub fn from_environment(env: &Environment) -> Self {
        Self {
            url: env.unleashed_url.clone(),
            username: env.unleashed_username.clone(),
            password: env.unleashed_password.clone(),
            ssid: env.unleashed_ssid.clone(),
            verify_tls: env.unleashed_has_valid_cert,
            timezone: env.timezone,
            rolling_voucher_duration_minutes: env.rolling_voucher_duration_minutes,
        }
    }
}

pub struct UnleashedAPI {
    session: Session,
    config: ApiConfig,
}

impl UnleashedAPI {
    /// Connect and log in, so bad settings fail at startup.
    pub async fn try_new(config: ApiConfig) -> Result<Self, String> {
        let session = Session::new(
            &config.url,
            &config.username,
            &config.password,
            config.verify_tls,
        )
        .map_err(|e| e.to_string())?;
        session.login().await.map_err(|e| e.to_string())?;
        Ok(Self { session, config })
    }

    fn now() -> i64 {
        Utc::now().timestamp()
    }

    async fn list_passes(&self) -> Result<Vec<GuestPass>, StatusCode> {
        let request = format!(
            "<ajax-request action='getconf' DECRYPT_X='true' updater='guest-list.{}' \
             comp='guest-list'><guest self-service='!true'/></ajax-request>",
            Utc::now().timestamp_millis()
        );
        let body = self.session.conf(&request).await.map_err(gateway)?;
        parse_guest_list(&body).map_err(|e| {
            error!("Could not read the guest list: {e}");
            StatusCode::BAD_GATEWAY
        })
    }

    /// Newest first.
    fn to_vouchers(&self, passes: &[GuestPass]) -> Vec<Voucher> {
        let now = Self::now();
        let mut sorted: Vec<&GuestPass> = passes.iter().collect();
        sorted.sort_by_key(|p| std::cmp::Reverse(p.create_time));
        sorted
            .into_iter()
            .map(|p| to_voucher(p, self.config.timezone, now))
            .collect()
    }

    pub async fn get_all_vouchers(&self) -> Result<VouchersGetResponse, StatusCode> {
        let data = self.to_vouchers(&self.list_passes().await?);
        Ok(VouchersGetResponse {
            offset: 0,
            limit: data.len() as u32,
            count: data.len() as u32,
            total_count: data.len() as u64,
            data,
        })
    }

    /// `filter` matches names case-insensitively.
    pub async fn get_vouchers(
        &self,
        request: &VouchersGetRequest,
    ) -> Result<VouchersGetResponse, StatusCode> {
        let mut data = self.to_vouchers(&self.list_passes().await?);
        if let Some(filter) = request.filter.as_deref().filter(|f| !f.is_empty()) {
            let filter = filter.to_lowercase();
            data.retain(|v| v.name.to_lowercase().contains(&filter));
        }
        let total_count = data.len() as u64;
        let data: Vec<Voucher> = data
            .into_iter()
            .skip(request.offset as usize)
            .take(request.limit as usize)
            .collect();
        Ok(VouchersGetResponse {
            offset: request.offset as u64,
            limit: request.limit,
            count: data.len() as u32,
            total_count,
            data,
        })
    }

    pub async fn get_voucher_details(&self, id: String) -> Result<Voucher, StatusCode> {
        let passes = self.list_passes().await?;
        let pass = passes
            .iter()
            .find(|p| p.id == id)
            .ok_or(StatusCode::NOT_FOUND)?;
        Ok(to_voucher(pass, self.config.timezone, Self::now()))
    }

    pub async fn get_newest_voucher(&self) -> Result<Voucher, StatusCode> {
        self.to_vouchers(&self.list_passes().await?)
            .into_iter()
            .next()
            .ok_or(StatusCode::NOT_FOUND)
    }

    pub async fn get_rolling_voucher(&self) -> Result<Option<Voucher>, StatusCode> {
        let passes = self.list_passes().await?;
        let now = Self::now();
        Ok(current_rolling(&passes, now).map(|p| to_voucher(p, self.config.timezone, now)))
    }

    pub async fn create_voucher(
        &self,
        request: &VouchersCreateRequest,
    ) -> Result<VouchersCreateResponse, StatusCode> {
        let params = CreateParams {
            count: request.count,
            name: request.name.clone(),
            minutes: request.time_limit_minutes,
            share_limit: request.authorized_guest_limit,
            key: request.code.clone().filter(|c| !c.is_empty()),
            remarks: request.remarks.clone(),
            ssid: self.config.ssid.clone(),
        };
        let form = build_form(&params).map_err(|e| {
            warn!("Rejected create request: {e}");
            StatusCode::BAD_REQUEST
        })?;

        let before: HashSet<String> = self
            .list_passes()
            .await?
            .into_iter()
            .map(|p| p.id)
            .collect();
        let body = self
            .session
            .form("mon_createguest.jsp", &form)
            .await
            .map_err(gateway)?;
        match parse_create_response(&body) {
            CreateOutcome::Accepted => {}
            CreateOutcome::KeyDuplicated(message) => {
                warn!("Create rejected: {message}");
                return Err(StatusCode::CONFLICT);
            }
            CreateOutcome::Failed(message) => {
                error!("Create failed: {message}");
                return Err(StatusCode::BAD_GATEWAY);
            }
        }

        // The response alone is not proof: the controller can answer OK and
        // create nothing.
        let created: Vec<GuestPass> = self
            .list_passes()
            .await?
            .into_iter()
            .filter(|p| !before.contains(&p.id))
            .collect();
        if created.is_empty() {
            error!("The controller accepted the create request but no new pass appeared");
            return Err(StatusCode::BAD_GATEWAY);
        }
        Ok(VouchersCreateResponse {
            vouchers: self.to_vouchers(&created),
        })
    }

    pub async fn check_rolling_voucher_ip(&self, ip: &str) -> Result<bool, StatusCode> {
        let passes = self.list_passes().await?;
        Ok(has_live_rolling_for_ip(&passes, ip, Self::now()))
    }

    pub async fn create_rolling_voucher(&self, ip: &str) -> Result<Voucher, StatusCode> {
        let request = VouchersCreateRequest {
            count: 1,
            name: rolling_name(Utc::now().with_timezone(&self.config.timezone), ip),
            authorized_guest_limit: None,
            time_limit_minutes: self.config.rolling_voucher_duration_minutes,
            code: None,
            remarks: None,
        };
        self.create_voucher(&request)
            .await?
            .vouchers
            .into_iter()
            .next()
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)
    }

    /// Deletes the passes among `ids` that exist. Ids are the controller's
    /// numeric pass ids; anything else is rejected before it reaches the XML.
    pub async fn delete_vouchers_by_ids(
        &self,
        ids: Vec<String>,
    ) -> Result<DeleteResponse, StatusCode> {
        let ids: Vec<String> = ids.into_iter().filter(|id| !id.is_empty()).collect();
        if ids.iter().any(|id| !id.chars().all(|c| c.is_ascii_digit())) {
            warn!("Rejected delete request with a non-numeric id");
            return Err(StatusCode::BAD_REQUEST);
        }
        let existing: HashSet<String> = self
            .list_passes()
            .await?
            .into_iter()
            .map(|p| p.id)
            .collect();
        let targets: Vec<String> = ids.into_iter().filter(|id| existing.contains(id)).collect();
        self.delete_ids(&targets).await
    }

    pub async fn delete_expired_vouchers(&self) -> Result<DeleteResponse, StatusCode> {
        let now = Self::now();
        let targets: Vec<String> = self
            .list_passes()
            .await?
            .into_iter()
            .filter(|p| is_expired(p, now))
            .map(|p| p.id)
            .collect();
        self.delete_ids(&targets).await
    }

    pub async fn delete_expired_rolling_vouchers(&self) -> Result<DeleteResponse, StatusCode> {
        let now = Self::now();
        let targets: Vec<String> = self
            .list_passes()
            .await?
            .into_iter()
            .filter(|p| is_rolling(p) && is_expired(p, now))
            .map(|p| p.id)
            .collect();
        self.delete_ids(&targets).await
    }

    async fn delete_ids(&self, ids: &[String]) -> Result<DeleteResponse, StatusCode> {
        if ids.is_empty() {
            return Ok(DeleteResponse {
                vouchers_deleted: 0,
            });
        }
        let guests: String = ids
            .iter()
            .map(|id| format!("<guest id='{id}'></guest>"))
            .collect();
        let request = format!(
            "<ajax-request action='delobj' updater='guest-list.{}' comp='guest-list'>{guests}\
             </ajax-request>",
            Utc::now().timestamp_millis()
        );
        self.session.conf(&request).await.map_err(gateway)?;
        info!("Deleted {} guest passes", ids.len());
        Ok(DeleteResponse {
            vouchers_deleted: ids.len() as u32,
        })
    }
}

fn gateway(e: SessionError) -> StatusCode {
    error!("{e}");
    StatusCode::BAD_GATEWAY
}
