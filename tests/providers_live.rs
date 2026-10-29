use browser_harbor::{
    domain::{settings::NetworkSettings, target::TargetId},
    providers::ProviderResolver,
    services::http::HttpClient,
};

#[tokio::test]
#[ignore = "requires access to official browser and runtime metadata endpoints"]
async fn resolves_every_supported_browser_target() {
    let http = HttpClient::new(&NetworkSettings::default()).unwrap();
    let resolver = ProviderResolver::new(http);
    let mut selections = Vec::new();
    for target in TargetId::ALL {
        let spec = target.spec();
        for channel in spec.channels {
            for cpu_build in spec.cpu_builds {
                selections.push((target, *channel, *cpu_build));
            }
        }
    }

    for (target, channel, cpu_build) in selections {
        let release = resolver
            .resolve(target, channel, cpu_build)
            .await
            .unwrap_or_else(|error| panic!("{}: {error}", target.display_name()));
        assert!(!release.version.is_empty());
        assert!(!release.artifact.urls.is_empty());
        println!(
            "{} {} {} {} {:?} {}",
            target.display_name(),
            release.channel.display_name(),
            release.version,
            release.artifact.file_name,
            release.artifact.integrity,
            release
                .artifact
                .urls
                .first()
                .map(String::as_str)
                .unwrap_or("")
        );
    }
}
