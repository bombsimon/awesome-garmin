//! The search module holds all types returned from <https://apps.garmin.com/api> and supports
//! searching their app catalog by keyword.
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

/// Search https://apps.garmin.com for Garmin apps by passing a keyword. The method will use the
/// pagination and iterate over all pages and store them in a vector which means that the result
/// size can blow up.
pub async fn search_garmin_apps(keyword: &str) -> anyhow::Result<Vec<ConnectIQApp>> {
    let client = reqwest::Client::new();

    let mut u = Url::parse(
        "https://apps.garmin.com/api/appsLibraryExternalServices/api/asw/apps/keywords",
    )?;

    let mut start_page_index = 0;
    let page_size = 30;

    let pairs = [
        ("keywords", keyword),
        ("pageSize", &page_size.to_string()),
        ("sortType", "mostPopular"),
    ];

    let mut apps = Vec::new();

    loop {
        u.query_pairs_mut()
            .clear()
            .extend_pairs(pairs)
            .append_pair("startPageIndex", &start_page_index.to_string());

        let page: ConnectIQ = client.get(u.as_str()).send().await?.json().await?;
        apps.extend(page.apps);

        if start_page_index + page_size >= page.total_count {
            break;
        }

        start_page_index += page_size;
    }

    Ok(apps)
}

pub async fn print_resource_urls(keyword: &str) -> anyhow::Result<()> {
    for app in search_garmin_apps(keyword).await? {
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
