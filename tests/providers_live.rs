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

    let mut failures = Vec::new();
    for (target, channel, cpu_build) in selections {
        let release = match resolver.resolve(target, channel, cpu_build).await {
            Ok(release) => release,
            Err(error) => {
                failures.push(format!(
                    "{} {} {}: {error:?}",
                    target.display_name(),
                    channel.display_name(),
                    cpu_build.display_name()
                ));
                continue;
            }
        };
        assert!(!release.version.is_empty());
        assert!(!release.artifact.urls.is_empty());
        println!(
            "{} {} {} {}",
            target.display_name(),
            release.channel.display_name(),
            release.version,
            release.artifact.file_name,
        );
    }
    assert!(
        failures.is_empty(),
        "Official metadata failures:\n{}",
        failures.join("\n")
    );
}
