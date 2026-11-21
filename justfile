_default:
    @just --list

sample count="4":
    cd awesome-generator && \
    GITHUB_TOKEN=$(gh auth token) \
    GITLAB_OAUTH_TOKEN=$(glab config get token --host gitlab.com) \
        cargo run generate-readme --sample {{count}} > ../README.md.tmp
    mv README.md.tmp README.md
