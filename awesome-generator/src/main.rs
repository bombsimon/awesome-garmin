use futures::StreamExt;
use gitlab::{api::AsyncQuery, AsyncGitlab};
use handlebars::{no_escape, Handlebars};
use octocrab::Octocrab;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, NoneAsEmptyString};
use std::{
    collections::{BTreeMap, HashMap},
    io::Read,
    sync::{Arc, Mutex},
};

/// The template that will be used to render the README.
const TEMPLATE: &str = include_str!("readme.md.hbs");

/// We do concurrent requests to GitHub and GitLab to speed up the process but we don't want to
/// hammer too hard so we limit the concurrent requests.
const MAX_CONCURRENT_REQUESTS: usize = 20;

/// If a repository has been inactive for more than 2 years we consider it to be inactive. These
/// might still be useful for reference but are put away under a separate menu to reduce noise.
const MAX_AGE_BEFORE_OLD: std::time::Duration = std::time::Duration::from_secs(86400 * 365 * 2);

/// A resource type is the type a resource can have mapped to the Garmin ecosystem. This also
/// includes some extra types for those projects not related to device app development.
/// https://developer.garmin.com/connect-iq/connect-iq-basics/app-types/
#[derive(Clone)]
enum ResourceType {
    WatchFace,
    DataField,
    Widget,
    DeviceApp,
    AudioContentProvider,
    Barrel,
    Tool,
    CompanionApp,
    Miscellaneous,
}

impl ResourceType {
    /// The map key is the key that will be used in the [`BTreeMap`] used for rendering the
    /// template file. Resources that have a section for old/inactive resources will have two keys,
    /// one prefixed `_active` and one prefixed `_inactive`.
    fn map_key(&self, is_old: bool) -> String {
        let key = match self {
            Self::WatchFace => "watch_face",
            ResourceType::DataField => "data_field",
            ResourceType::Widget => "widget",
            ResourceType::DeviceApp => "device_app",
            ResourceType::AudioContentProvider => return "audio_content_provider".to_string(),
            ResourceType::Barrel => return "barrel".to_string(),
            ResourceType::Tool => return "tool".to_string(),
            ResourceType::CompanionApp => return "companion_app".to_string(),
            ResourceType::Miscellaneous => return "miscellaneous".to_string(),
        };

        if is_old {
            format!("{}_inactive", key)
        } else {
            format!("{}_active", key)
        }
    }
}

/// The TomlFileItem represents a single row in the TOML file that is used to define resources.
/// Currently it only contains support for a custom name and a description. This is mostly useful
/// if the resource is not a GitHub or GitLab repository.
#[serde_as]
#[derive(Debug, Serialize, Deserialize)]
struct TomlFileItem {
    #[serde_as(as = "NoneAsEmptyString")]
    #[serde(default)]
    name: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    #[serde(default)]
    description: Option<String>,
}

/// The TOML file holds all resources that should be generated in the final README.
#[derive(Debug, Deserialize)]
struct TomlFile {
    watch_faces: HashMap<String, TomlFileItem>,
    data_fields: HashMap<String, TomlFileItem>,
    widgets: HashMap<String, TomlFileItem>,
    device_apps: HashMap<String, TomlFileItem>,
    audio_content_providers: HashMap<String, TomlFileItem>,
    barrels: HashMap<String, TomlFileItem>,
    tools: HashMap<String, TomlFileItem>,
    companion_apps: HashMap<String, TomlFileItem>,
    miscellaneous: HashMap<String, TomlFileItem>,
}

/// A [`GarminResource`] is the resource that is populated after resolving the TOML file contents
/// and fetching additional information from an API. It holds all the data used to render the
/// README items.
#[derive(Debug, Serialize)]
struct GarminResource {
    name: String,
    description: Option<String>,
    url: String,
    #[serde(with = "ymd_date")]
    last_updated: Option<chrono::DateTime<chrono::Utc>>,
    is_archived: bool,
}

/// The data that is passed to render the template. It contains all the resolved Garmin resources
/// grouped by type and a timestamp to set when the file was generated.
#[derive(Serialize)]
struct Template {
    resources: BTreeMap<String, Vec<GarminResource>>,
    updated_at: String,
}

/// The GitLab client does not come with pre-defined types, instead it will deserialize to whatever
/// type the user define. This is the only data we're currently interested in.
#[derive(Debug, Deserialize)]
struct GitLabProject {
    name: String,
    description: Option<String>,
    last_activity_at: chrono::DateTime<chrono::Utc>,
    archived: bool,
}

#[tokio::main]
async fn main() -> Result<(), &'static str> {
    let mut toml_content = String::new();
    std::fs::File::open("awesome.toml")
        .expect("Failed to open awesome.toml")
        .read_to_string(&mut toml_content)
        .expect("Failed to read awesome.toml");

    let resources: TomlFile = toml::from_str(&toml_content).expect("Failed to parse TOML");
    let octocrab = Arc::new(
        octocrab::OctocrabBuilder::new()
            .personal_token(std::env::var("GITHUB_TOKEN").unwrap())
            .build()
            .unwrap(),
    );
    let glab = Arc::new(
        gitlab::GitlabBuilder::new("gitlab.com", std::env::var("GITLAB_TOKEN").unwrap())
            .build_async()
            .await
            .unwrap(),
    );

    let data: Arc<Mutex<BTreeMap<String, Vec<GarminResource>>>> =
        Arc::new(Mutex::new(BTreeMap::new()));

    let resource_types = vec![
        (ResourceType::WatchFace, resources.watch_faces),
        (ResourceType::DataField, resources.data_fields),
        (ResourceType::Widget, resources.widgets),
        (ResourceType::DeviceApp, resources.device_apps),
        (
            ResourceType::AudioContentProvider,
            resources.audio_content_providers,
        ),
        (ResourceType::Barrel, resources.barrels),
        (ResourceType::Tool, resources.tools),
        (ResourceType::CompanionApp, resources.companion_apps),
        (ResourceType::Miscellaneous, resources.miscellaneous),
    ];

    let mut futures = Vec::new();
    for (resource_type, resources) in resource_types {
        for (resource_url, resource) in resources {
            futures.push(update_resource(
                resource_type.clone(),
                resource_url,
                resource,
                octocrab.clone(),
                glab.clone(),
                data.clone(),
            ));
        }
    }

    let stream = futures::stream::iter(futures).buffer_unordered(MAX_CONCURRENT_REQUESTS);
    stream.collect::<Vec<_>>().await;

    let mut hb = Handlebars::new();
    hb.register_escape_fn(no_escape);
    hb.register_template_string("readme", TEMPLATE).unwrap();

    {
        let mut d = data.lock().unwrap();
        for (_, v) in d.iter_mut() {
            sorted_resources(v);
        }
    }

    let template = Template {
        resources: Arc::try_unwrap(data).unwrap().into_inner().unwrap(),
        updated_at: chrono::Utc::now().format("%Y-%m-%d").to_string(),
    };

    println!("{}", hb.render("readme", &template).unwrap());

    Ok(())
}

/// The resources will be sorted by date - if they have any, and then by name.
fn sorted_resources(resources: &mut [GarminResource]) {
    resources.sort_by(|a, b| match (a.last_updated, b.last_updated) {
        (None, None) => a.name.cmp(&b.name),
        (Some(a), Some(b)) => b.cmp(&a),
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
    });
}

/// A single resources is updated based on the URL. It will be added to the `BTreeMap` once
/// resolved and not return any data.
async fn update_resource(
    resource_type: ResourceType,
    resource_url: String,
    resource: TomlFileItem,
    octocrab: Arc<Octocrab>,
    glab: Arc<AsyncGitlab>,
    data: Arc<Mutex<BTreeMap<String, Vec<GarminResource>>>>,
) {
    eprintln!("Updating {}", resource_url);

    let (resource, is_old) = if resource_url.contains("github.com") {
        update_github_resource(resource_url, &resource, octocrab).await
    } else if resource_url.contains("gitlab.com") {
        update_gitlab_resource(resource_url, &resource, glab).await
    } else if let Some(name) = resource.name {
        (
            Some(GarminResource {
                name,
                description: resource.description,
                url: resource_url,
                last_updated: None,
                is_archived: false,
            }),
            true,
        )
    } else {
        return;
    };

    if let Some(resource) = resource {
        let key = resource_type.map_key(is_old);
        let mut m = data.lock().unwrap();
        let elem = m.entry(key).or_default();
        elem.push(resource)
    }
}

/// Will poll the GitHub API and fetch information about the repo.
async fn update_github_resource(
    resource_url: String,
    resource: &TomlFileItem,
    octocrab: Arc<octocrab::Octocrab>,
) -> (Option<GarminResource>, bool) {
    let u = url::Url::parse(&resource_url).unwrap();
    let mut owner_repo = u.path().strip_prefix('/').unwrap().split('/');
    let owner = owner_repo.next().unwrap();
    let repo = owner_repo.next().unwrap();
    let result = match octocrab.repos(owner, repo).get().await {
        Ok(result) => result,
        Err(err) => {
            eprintln!("⚠️ Could not get {}: {err}", resource_url);
            return (None, false);
        }
    };

    let garmin_resource = GarminResource {
        name: repo.to_string(),
        description: Some(
            resource
                .description
                .clone()
                .unwrap_or(result.description.unwrap_or_default()),
        ),
        url: resource_url.to_string(),
        last_updated: result.pushed_at,
        is_archived: result.archived.unwrap_or_default(),
    };

    let is_old = if let Some(pushed_at) = result.pushed_at {
        pushed_at < chrono::Utc::now() - MAX_AGE_BEFORE_OLD
    } else {
        false
    };

    (Some(garmin_resource), is_old)
}

/// Will poll the GitLab API and fetch information about the repo.
async fn update_gitlab_resource(
    resource_url: String,
    resource: &TomlFileItem,
    glab: Arc<gitlab::AsyncGitlab>,
) -> (Option<GarminResource>, bool) {
    let u = url::Url::parse(&resource_url).unwrap();
    let owner_repo = u.path().strip_prefix('/').unwrap();
    let endpoint = gitlab::api::projects::Project::builder()
        .project(owner_repo)
        .build()
        .unwrap();
    let result: GitLabProject = match endpoint.query_async(&*glab).await {
        Ok(result) => result,
        Err(err) => {
            eprintln!("⚠️ Could not get {}: {err}", resource_url);
            return (None, false);
        }
    };

    let garmin_resource = GarminResource {
        name: result.name,
        description: Some(
            resource
                .description
                .clone()
                .unwrap_or(result.description.unwrap_or_default()),
        ),
        url: resource_url.to_string(),
        last_updated: Some(result.last_activity_at),
        is_archived: result.archived,
    };

    (
        Some(garmin_resource),
        result.last_activity_at < chrono::Utc::now() - MAX_AGE_BEFORE_OLD,
    )
}

/// [`ymd_date`] implements a serializer to show a more condensed date in the README. It will only
/// show YYYY-MM-DD.
mod ymd_date {
    use serde::{self, Serializer};

    const FORMAT: &str = "%Y-%m-%d";

    pub fn serialize<S>(
        date: &Option<chrono::DateTime<chrono::Utc>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match date {
            Some(date) => {
                let s = format!("{}", date.format(FORMAT));
                serializer.serialize_str(&s)
            }
            None => serializer.serialize_str(""),
        }
    }
}
