use futures::StreamExt;
use gitlab::{api::AsyncQuery, AsyncGitlab};
use handlebars::{no_escape, Context, Handlebars, Helper, HelperResult, Output, RenderContext};
use octocrab::Octocrab;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, NoneAsEmptyString};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::Read,
    sync::{Arc, Mutex},
};

pub mod search;

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
#[derive(Clone, Debug)]
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

impl TryFrom<String> for ResourceType {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "1" => Ok(Self::WatchFace),
            "2" => Ok(Self::DeviceApp),
            "3" => Ok(Self::Widget),
            "4" => Ok(Self::DataField),
            "5" => Ok(Self::AudioContentProvider),
            id => Err(anyhow::anyhow!("invalid type id: {}", id)),
        }
    }
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
    star_count: Option<u32>,
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
    star_count: u32,
}

/// Generate the README based on all the contents in `awesome.toml`. If the element is a link to
/// GitHub or GitLab their API will be called to fetch description and information about last
/// activity.
pub async fn generate_readme() -> anyhow::Result<()> {
    let resources = read_toml_file()?;
    let octocrab = Arc::new(
        octocrab::OctocrabBuilder::new()
            .personal_token(std::env::var("GITHUB_TOKEN")?)
            .build()?,
    );
    let glab = Arc::new(
        gitlab::GitlabBuilder::new("gitlab.com", std::env::var("GITLAB_TOKEN")?)
            .build_async()
            .await?,
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
    hb.register_helper("resourceList", Box::new(resource_list_helper));

    {
        let mut d = data.lock().unwrap();
        for (_, v) in d.iter_mut() {
            sorted_resources(v);
        }
    }

    let template = Template {
        resources: Arc::try_unwrap(data).unwrap().into_inner()?,
        updated_at: chrono::Utc::now().format("%Y-%m-%d").to_string(),
    };

    println!("{}", hb.render("readme", &template)?);

    Ok(())
}

fn resource_list_helper(
    h: &Helper,
    _: &Handlebars,
    _: &Context,
    _: &mut RenderContext,
    out: &mut dyn Output,
) -> HelperResult {
    let mut output = String::new();

    let show_description = h
        .param(2)
        .map_or(true, |p| p.value().as_bool().unwrap_or(true));

    let active = h.param(0).unwrap().value();
    output.push_str(&resources_to_str(active, show_description));

    if let Some(inactive_list) = h.param(1) {
        let inactive = resources_to_str(inactive_list.value(), show_description);
        if !inactive.is_empty() {
            output.push_str(&format!(
                r#"
### Older resources

<details>
  <summary>Click to expand</summary>

{inactive}

</details>"#
            ));
        }
    }

    out.write(output.as_str())?;

    Ok(())
}

fn resources_to_str(resources: &serde_json::Value, show_description: bool) -> String {
    let mut output = String::new();

    if let Some(active_list) = resources.as_array() {
        if show_description {
            output.push_str("| Name | Description | Last&nbsp;updated | Stars |\n");
            output.push_str("| ---- | ----------- | ----------------- | ----- |\n");
        } else {
            output.push_str("| Name | Last&nbsp;updated | Stars |\n");
            output.push_str("| ---- | ----------------- | ----- |\n");
        }

        for resource in active_list {
            if let Some(name) = resource.get("name").and_then(|n| n.as_str()) {
                let url = resource.get("url").and_then(|u| u.as_str()).unwrap_or("#");
                let description = resource
                    .get("description")
                    .and_then(|d| d.as_str().map(|v| v.replace("|", "-")));
                let star_count = resource.get("star_count").and_then(|s| s.as_u64());
                let last_updated = resource.get("last_updated").and_then(|l| l.as_str());
                let is_archived = resource.get("is_archived").and_then(|a| a.as_bool());

                output.push_str(&format!("| [{name}]({url}) "));

                if show_description {
                    if let Some(description) = description {
                        output.push_str(&format!("| {description} "));
                    } else {
                        output.push_str("| ");
                    }
                }

                let is_archived = if let Some(true) = is_archived {
                    "🗄️"
                } else {
                    ""
                };

                if let Some(date) = last_updated {
                    output.push_str(&format!("| {date}&nbsp;{is_archived} "));
                } else {
                    output.push_str("| {is_archived} ");
                }

                if let Some(stars) = star_count {
                    output.push_str("| ");
                    if stars > 0 {
                        output.push_str(&format!("⭐{stars} "));
                    }
                }

                output.push_str("|\n");
            }
        }
    }

    output
}

/// Read the toml file and return the prased file as a [`TomlFile`].
fn read_toml_file() -> anyhow::Result<TomlFile> {
    let mut toml_content = String::new();
    std::fs::File::open("awesome.toml")
        .expect("Failed to open awesome.toml")
        .read_to_string(&mut toml_content)
        .expect("Failed to read awesome.toml");

    Ok(toml::from_str(&toml_content)?)
}

/// The resources will be sorted by date - if they have any, and then by name.
fn sorted_resources(resources: &mut [GarminResource]) {
    resources.sort_by(|a, b| match (a.last_updated, b.last_updated) {
        (None, None) => a.name.cmp(&b.name),
        (Some(u1), Some(u2)) => u2.cmp(&u1).then(a.name.cmp(&b.name)),
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
                star_count: None,
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
        Err(octocrab::Error::GitHub { source, .. }) => {
            eprintln!("⚠️ Could not get {resource_url}: {}", source.message);
            return (None, false);
        }
        Err(err) => {
            eprintln!("⚠️ Could not get {resource_url}: {err}");
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
        star_count: result.stargazers_count,
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
    let result: GitLabProject = match endpoint.query_async(glab.as_ref()).await {
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
        star_count: Some(result.star_count),
    };

    (
        Some(garmin_resource),
        result.last_activity_at < chrono::Utc::now() - MAX_AGE_BEFORE_OLD,
    )
}

/// Compare what's in the `awesome.toml` file with all the found search results based on the given
/// `keyword`. This is a manual but easy way to see which resources are not listed yet.
///
/// The output will look just like the toml file to make it easy to compare or copy/paste.
pub async fn compare(keyword: &str) -> anyhow::Result<()> {
    let resources = read_toml_file()?;
    let toml_file_keys = vec![
        resources.watch_faces.keys().collect::<HashSet<_>>(),
        resources.data_fields.keys().collect::<HashSet<_>>(),
        resources.widgets.keys().collect::<HashSet<_>>(),
        resources.device_apps.keys().collect::<HashSet<_>>(),
        resources
            .audio_content_providers
            .keys()
            .collect::<HashSet<_>>(),
        resources.barrels.keys().collect::<HashSet<_>>(),
        resources.tools.keys().collect::<HashSet<_>>(),
        resources.companion_apps.keys().collect::<HashSet<_>>(),
        resources.miscellaneous.keys().collect::<HashSet<_>>(),
    ];

    let tomle_file_urls = toml_file_keys
        .into_iter()
        .flatten()
        .map(|i| i.to_owned())
        .collect::<HashSet<String>>();

    // Store each app type in a separate `HashSet` so we can print it properly.
    let mut watch_faces = HashSet::new();
    let mut data_fields = HashSet::new();
    let mut widgets = HashSet::new();
    let mut device_apps = HashSet::new();
    let mut audio_content_providers = HashSet::new();

    let mut s = crate::search::ConnectIQSearch::new(keyword.to_string());
    while let Some(app) = s.next().await {
        if app.website_url.is_empty() {
            continue;
        }

        if !app.website_url.starts_with("https://github")
            && !app.website_url.starts_with("https://gitlab")
        {
            continue;
        }

        // A lot of URLs goes to paths in a multi repo or have a trailing slash. This list only
        // contains full repos so we only care about the repo base URL.
        let parsed_url = url::Url::parse(&app.website_url)?;
        let repo_base_url = format!(
            "{}://{}{}",
            parsed_url.scheme(),
            parsed_url.host_str().unwrap(),
            parsed_url
                .path()
                .split('/')
                .take(3)
                .collect::<Vec<_>>()
                .join("/")
        );

        if tomle_file_urls.contains(&repo_base_url) {
            continue;
        }

        let resource_type = ResourceType::try_from(app.type_id)?;
        match resource_type {
            ResourceType::WatchFace => watch_faces.insert(app.website_url),
            ResourceType::DataField => data_fields.insert(app.website_url.clone()),
            ResourceType::Widget => widgets.insert(app.website_url.clone()),
            ResourceType::DeviceApp => device_apps.insert(app.website_url.clone()),
            ResourceType::AudioContentProvider => {
                audio_content_providers.insert(app.website_url.clone())
            }
            _ => unreachable!(),
        };
    }

    println!(
        "Found {} URLs not in list\n",
        watch_faces.len()
            + data_fields.len()
            + widgets.len()
            + device_apps.len()
            + audio_content_providers.len()
    );

    for (app_set, header) in [
        (watch_faces, "watch_faces"),
        (data_fields, "data_fields"),
        (widgets, "widgets"),
        (device_apps, "device_apps"),
        (audio_content_providers, "audio_content_providers"),
    ] {
        if !app_set.is_empty() {
            println!("[{header}]");
            for u in app_set {
                println!("\"{u}\" = {{}}");
            }

            println!();
        }
    }

    Ok(())
}

/// [`ymd_date`] implements a serializer to show a more condensed date in the README. It will only
/// show YYYY-MM-DD.
mod ymd_date {
    use serde::{self, Serializer};

    const FORMAT: &str = "%Y&#x2011;%m&#x2011;%d";

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

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use crate::{sorted_resources, GarminResource, TomlFileItem};

    #[test]
    fn same_updated_should_sort_on_name() {
        let t0 = chrono::Utc::now();
        let t1 = t0 - std::time::Duration::from_secs(5);

        let mut r = vec![
            GarminResource {
                name: "Name A".to_string(),
                last_updated: Some(t1),
                description: None,
                url: "#".to_string(),
                is_archived: false,
                star_count: None,
            },
            GarminResource {
                name: "Name C".to_string(),
                last_updated: Some(t0),
                description: None,
                url: "#".to_string(),
                is_archived: false,
                star_count: None,
            },
            GarminResource {
                name: "Name B".to_string(),
                last_updated: Some(t0),
                description: None,
                url: "#".to_string(),
                is_archived: false,
                star_count: None,
            },
        ];

        sorted_resources(&mut r);

        let names = r.into_iter().map(|n| n.name).collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                "Name B".to_string(),
                "Name C".to_string(),
                "Name A".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn test_github() {
        let empy_toml = TomlFileItem {
            name: None,
            description: None,
        };
        let octocrab = Arc::new(
            octocrab::OctocrabBuilder::new()
                .personal_token(std::env::var("GITHUB_TOKEN").unwrap())
                .build()
                .unwrap(),
        );

        let url = "https://github.com/bombsimon/garmin-seaside";
        let (resource, _) =
            super::update_github_resource(url.to_string(), &empy_toml, octocrab.clone()).await;

        assert!(resource.is_some());

        let resource_data = resource.unwrap();
        assert!(resource_data.star_count.unwrap_or(0) >= 1);
        assert!(resource_data.description.is_some());
        assert_eq!(resource_data.name, "garmin-seaside".to_string());
        assert_eq!(resource_data.url, url.to_string());
        assert!(!resource_data.is_archived);
    }

    #[tokio::test]
    async fn test_gitlab() {
        let empy_toml = TomlFileItem {
            name: None,
            description: None,
        };
        let glab = Arc::new(
            gitlab::GitlabBuilder::new("gitlab.com", std::env::var("GITLAB_TOKEN").unwrap())
                .build_async()
                .await
                .unwrap(),
        );

        let url = "https://gitlab.com/knusprjg/plotty-mcclockface";
        let (resource, _) =
            super::update_gitlab_resource(url.to_string(), &empy_toml, glab.clone()).await;

        assert!(resource.is_some());

        let resource_data = resource.unwrap();
        assert!(resource_data.star_count.unwrap_or(0) >= 1);
        assert!(resource_data.description.is_some());
        assert_eq!(resource_data.name, "Plotty McClockface".to_string());
        assert_eq!(resource_data.url, url.to_string());
        assert!(!resource_data.is_archived);
    }
}
