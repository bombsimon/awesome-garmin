use futures::StreamExt;
use handlebars::{no_escape, Handlebars};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, NoneAsEmptyString};
use std::{
    collections::{BTreeMap, HashMap},
    io::Read,
    sync::Arc,
};

const TEMPLATE: &str = include_str!("readme.md.hbs");
const MAX_CONCURRENT_REQUESTS: usize = 20;

#[serde_as]
#[derive(Debug, Serialize, Deserialize)]
struct Resource {
    #[serde_as(as = "NoneAsEmptyString")]
    #[serde(default)]
    name: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    #[serde(default)]
    description: Option<String>,
    #[serde(skip_deserializing)]
    url: String,
    #[serde(skip_deserializing)]
    owner: Option<String>,
    #[serde(skip_deserializing)]
    repo: Option<String>,
    #[serde(skip_deserializing, with = "ymd_date")]
    last_updated: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    is_archived: bool,
}

#[derive(Debug, Deserialize)]
struct Resources {
    watch_faces: HashMap<String, Resource>,
    data_fields: HashMap<String, Resource>,
    widgets: HashMap<String, Resource>,
    device_apps: HashMap<String, Resource>,
    audio_content_providers: HashMap<String, Resource>,
    barrels: HashMap<String, Resource>,
    tools: HashMap<String, Resource>,
    companion_apps: HashMap<String, Resource>,
    miscellaneous: HashMap<String, Resource>,
}

#[derive(Serialize)]
struct Template<'a> {
    resources: BTreeMap<&'a str, Vec<Resource>>,
    updated_at: String,
}

#[tokio::main]
async fn main() -> Result<(), &'static str> {
    let mut toml_content = String::new();
    std::fs::File::open("awesome.toml")
        .expect("Failed to open awesome.toml")
        .read_to_string(&mut toml_content)
        .expect("Failed to read awesome.toml");

    let mut resources: Resources = toml::from_str(&toml_content).expect("Failed to parse TOML");
    let octocrab = Arc::new(
        octocrab::OctocrabBuilder::new()
            .personal_token(std::env::var("GITHUB_TOKEN").unwrap())
            .build()
            .unwrap(),
    );

    let mut futures = Vec::new();
    for resources in [
        &mut resources.watch_faces,
        &mut resources.data_fields,
        &mut resources.widgets,
        &mut resources.device_apps,
        &mut resources.audio_content_providers,
        &mut resources.barrels,
        &mut resources.tools,
        &mut resources.companion_apps,
        &mut resources.miscellaneous,
    ] {
        for (resource_url, resource) in resources.iter_mut() {
            let oc = octocrab.clone();

            futures.push(async move {
                update_resource(resource_url, resource, oc).await;
            });
        }
    }

    let stream = futures::stream::iter(futures).buffer_unordered(MAX_CONCURRENT_REQUESTS);
    stream.collect::<Vec<_>>().await;

    let mut data = BTreeMap::new();
    data.insert("watch_face", sorted_resources(resources.watch_faces));
    data.insert("data_field", sorted_resources(resources.data_fields));
    data.insert("widget", sorted_resources(resources.widgets));
    data.insert("device_app", sorted_resources(resources.device_apps));
    data.insert(
        "audio_content_provider",
        sorted_resources(resources.audio_content_providers),
    );
    data.insert("barrel", sorted_resources(resources.barrels));
    data.insert("tool", sorted_resources(resources.tools));
    data.insert("companion_app", sorted_resources(resources.companion_apps));
    data.insert("miscellaneous", sorted_resources(resources.miscellaneous));

    let mut hb = Handlebars::new();
    hb.register_escape_fn(no_escape);
    hb.register_template_string("readme", TEMPLATE).unwrap();

    let template = Template {
        resources: data,
        updated_at: chrono::Utc::now().format("%Y-%m-%d").to_string(),
    };

    println!("{}", hb.render("readme", &template).unwrap());

    Ok(())
}

fn sorted_resources(resources: HashMap<String, Resource>) -> Vec<Resource> {
    let mut r = resources.into_values().collect::<Vec<_>>();

    r.sort_by(|a, b| match (a.last_updated, b.last_updated) {
        (None, None) => match (&a.name, &b.name) {
            (Some(a), Some(b)) => a.cmp(b),
            _ => std::cmp::Ordering::Equal,
        },
        (Some(a), Some(b)) => b.cmp(&a),
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
    });

    r
}

async fn update_resource(
    resource_url: &str,
    resource: &mut Resource,
    octocrab: Arc<octocrab::Octocrab>,
) {
    eprintln!("Updating {resource_url}");

    resource.url = resource_url.to_string();

    if !resource_url.contains("github.com") {
        return;
    }

    let u = url::Url::parse(resource_url).unwrap();
    let mut owner_repo = u.path().strip_prefix('/').unwrap().split('/');
    let owner = owner_repo.next().unwrap();
    let repo = owner_repo.next().unwrap();
    let result = match octocrab.repos(owner, repo).get().await {
        Ok(result) => result,
        Err(err) => {
            eprintln!("⚠️ Could not get {resource_url}: {err}");
            return;
        }
    };

    if resource.description.is_none() {
        resource.description = result.description;
    }

    resource.owner = Some(owner.to_string());
    resource.repo = Some(repo.to_string());
    resource.last_updated = result.pushed_at;

    if let Some(archived) = result.archived {
        resource.is_archived = archived;
    }
}

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
