//! The search module holds all types returned from <https://apps.garmin.com/api> and supports
//! searching their app catalog by keyword.
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::task::{ready, Poll};

use futures::StreamExt;
use pin_project::pin_project;
use prettytable::{row, Table};
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

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ConnectIQFileSize {
    pub internal_version_number: i64,
    pub byte_count_by_device_type_id: HashMap<i64, i64>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ConnecTIQSettingsAvailability {
    pub internal_version_number: i64,
    pub availability_by_device_type_id: HashMap<i64, bool>,
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
    pub file_size_info: ConnectIQFileSize,
    pub settings_availability_info: ConnecTIQSettingsAvailability,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ConnectIQ {
    pub total_count: usize,
    pub apps: Vec<ConnectIQApp>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ConnectIQDeviceType {
    pub additional_names: Vec<String>,
    pub id: String,
    pub image_url: String,
    pub name: String,
    pub part_number: String,
    pub url_name: String,
}

#[pin_project]
pub struct ConnectIQSearch {
    client: std::sync::Arc<reqwest::Client>,
    keyword: String,
    apps: VecDeque<ConnectIQApp>,
    start_page_index: usize,
    has_more_pages: bool,

    future: Option<Pin<Box<dyn Future<Output = anyhow::Result<ConnectIQ>>>>>,
}

impl ConnectIQSearch {
    const PAGE_SIZE: usize = 30;

    pub fn new(keyword: String) -> Self {
        Self {
            client: std::sync::Arc::new(reqwest::Client::new()),
            apps: VecDeque::new(),
            start_page_index: 0,
            has_more_pages: true,
            keyword,
            future: None,
        }
    }

    pub fn fetch_next_page(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<()> {
        if self.future.is_none() {
            let fut = fetch_page(
                self.client.clone(),
                self.keyword.clone(),
                Self::PAGE_SIZE,
                self.start_page_index,
            );

            self.future = Some(Box::pin(fut));
        }

        let p = self.project();
        let Some(fut) = (*p.future).as_mut() else {
            return Poll::Ready(());
        };

        match fut.as_mut().poll(cx) {
            Poll::Ready(Ok(page)) => {
                p.apps.extend(page.apps);

                *p.has_more_pages = *p.start_page_index + Self::PAGE_SIZE < page.total_count;
                *p.start_page_index += Self::PAGE_SIZE;
                *p.future = None;

                Poll::Ready(())
            }
            Poll::Ready(_) => {
                *p.has_more_pages = false;
                Poll::Ready(())
            }
            Poll::Pending => Poll::Pending,
        }
    }

    pub async fn device_types(&self) -> anyhow::Result<Vec<ConnectIQDeviceType>> {
        let u = Url::parse(
            "https://apps.garmin.com/api/appsLibraryExternalServices/api/asw/deviceTypes",
        )?;

        Ok(self.client.get(u.as_str()).send().await?.json().await?)
    }
}
async fn fetch_page(
    client: std::sync::Arc<reqwest::Client>,
    keyword: String,
    page_size: usize,
    start_page_index: usize,
) -> anyhow::Result<ConnectIQ> {
    let mut u = Url::parse(
        "https://apps.garmin.com/api/appsLibraryExternalServices/api/asw/apps/keywords",
    )?;

    let pairs = [
        ("keywords", keyword.as_str()),
        ("pageSize", &page_size.to_string()),
        ("sortType", "mostPopular"),
    ];

    u.query_pairs_mut()
        .clear()
        .extend_pairs(pairs)
        .append_pair("startPageIndex", start_page_index.to_string().as_str());

    Ok(client.get(u.as_str()).send().await?.json().await?)
}

impl futures::Stream for ConnectIQSearch {
    type Item = ConnectIQApp;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(item) = self.as_mut().apps.pop_front() {
                return Poll::Ready(Some(item));
            }

            if !self.has_more_pages {
                return Poll::Ready(None);
            }

            ready!(self.as_mut().fetch_next_page(cx))
        }
    }
}

pub async fn print_resource_urls(keyword: &str) -> anyhow::Result<()> {
    let mut s = ConnectIQSearch::new(keyword.to_string());

    let mut table = Table::new();
    table.set_format(*prettytable::format::consts::FORMAT_NO_BORDER_LINE_SEPARATOR);
    table.set_titles(row!["Change date", "Type", "URL"]);

    while let Some(app) = s.next().await {
        if !app.website_url.is_empty() {
            let resource_type = format!("{:?}", crate::ResourceType::try_from(app.type_id)?);
            table.add_row(row![app.changed_date, resource_type, app.website_url]);
        }
    }

    table.printstd();

    Ok(())
}
