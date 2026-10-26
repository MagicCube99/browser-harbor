use browser_harbor::{
    domain::{release::CpuBuild, settings::NetworkSettings, target::TargetId},
    providers::ProviderResolver,
    services::http::HttpClient,
};

#[tokio::test]
#[ignore = "requires access to official browser and runtime metadata endpoints"]
async fn resolves_every_supported_browser_target() {
    let http = HttpClient::new(&NetworkSettings::default()).unwrap();
    let resolver = ProviderResolver::new(http);
    let mut selections = TargetId::ALL
        .into_iter()
        .map(|target| {
            let spec = target.spec();
            (target, spec.default_channel, spec.default_cpu_build)
        })
        .collect::<Vec<_>>();
    selections.push((
        TargetId::Iceweasel,
        browser_harbor::domain::release::ReleaseChannel::Release,
        CpuBuild::X64,
    ));

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
