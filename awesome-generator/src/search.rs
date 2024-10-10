//! The search module holds all types returned from <https://apps.garmin.com/api> and supports
//! searching their app catalog by keyword.
use std::collections::VecDeque;

use serde_with::{formats::Flexible, serde_as, TimestampMilliSeconds};
use url::Url;

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ConnectIQLocale {
    pub locale: String,
    pub name: String,
    pub description: String,
    pub whats_new: String,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ConnectIQDeveloper {
    pub full_name: Option<String>,
    pub developer_display_name: String,
    pub logo_url: Option<String>,
    pub logo_url_dark: Option<String>,
    pub trusted_developer: bool,
}

#[serde_as]
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ConnectIQApp {
    pub id: String,
    pub developer_id: String,
    pub type_id: String,
    pub website_url: String,
    pub video_url: String,
    pub privacy_policy_url: String,
    pub support_email_address: String,
    pub app_localizations: Vec<ConnectIQLocale>,
    pub status: String,
    pub ios_app_url: String,
    pub android_app_url: String,
    pub icon_file_id: String,
    pub latest_external_version: String,
    pub latest_internal_version: i64,
    pub download_count: i64,
    #[serde_as(as = "TimestampMilliSeconds<i64, Flexible>")]
    pub changed_date: chrono::DateTime<chrono::Utc>,
    pub average_rating: f32,
    pub review_count: i64,
    pub category_id: String,
    pub compatible_device_type_ids: Vec<String>,
    pub has_trial_mode: bool,
    pub auth_flow_support: i64,
    pub permissions: Vec<String>,
    pub latest_version_auto_migrated: bool,
    pub screenshot_file_ids: Vec<String>,
    pub developer: ConnectIQDeveloper,
    pub payment_model: i64,
    // pub file_size_info: ConnectIQFileSize
    // pub settings_availability_info: ConnectIQFileSize
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ConnectIQ {
    pub total_count: i64,
    pub apps: Vec<ConnectIQApp>,
}

pub struct ConnectIQSearch {
    client: reqwest::Client,
    keyword: String,
    apps: VecDeque<ConnectIQApp>,
    start_page_index: i64,
    has_more_pages: bool,
}

impl ConnectIQSearch {
    pub fn new(keyword: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            apps: VecDeque::new(),
            start_page_index: 0,
            has_more_pages: true,
            keyword,
        }
    }

    pub async fn next_item(&mut self) -> Option<ConnectIQApp> {
        if self.apps.is_empty() && self.has_more_pages {
            self.next_page().await.ok();
        }

        self.apps.pop_front()
    }

    async fn next_page(&mut self) -> anyhow::Result<()> {
        let mut u = Url::parse(
            "https://apps.garmin.com/api/appsLibraryExternalServices/api/asw/apps/keywords",
        )?;

        let page_size = 30;

        let pairs = [
            ("keywords", self.keyword.as_str()),
            ("pageSize", &30.to_string()),
            ("sortType", "mostPopular"),
        ];

        u.query_pairs_mut()
            .clear()
            .extend_pairs(pairs)
            .append_pair("startPageIndex", &self.start_page_index.to_string());

        let page: ConnectIQ = self.client.get(u.as_str()).send().await?.json().await?;
        self.apps = VecDeque::from(page.apps);

        self.has_more_pages = self.start_page_index + page_size < page.total_count;
        self.start_page_index += page_size;

        Ok(())
    }
}

pub async fn print_resource_urls(keyword: &str) -> anyhow::Result<()> {
    let mut s = ConnectIQSearch::new(keyword.to_string());

    while let Some(app) = s.next_item().await {
        if !app.website_url.is_empty() {
            let resource_type = format!("{:?}", crate::ResourceType::try_from(app.type_id)?);

            println!(
                "{} - {:<20} - {}",
                app.changed_date, resource_type, app.website_url
            );
        }
    }

    Ok(())
}
