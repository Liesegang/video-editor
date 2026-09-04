fn main() -> Result<(), Box<dyn std::error::Error>> {
    if ocio_rs::is_stub_build() {
        return Err("the real OpenColorIO gate linked ocio-rs in stub mode".into());
    }

    let version = ocio_rs::version().ok_or("real OpenColorIO did not report a runtime version")?;
    if version != "2.5.2" {
        return Err(format!(
            "the real OpenColorIO gate linked unexpected runtime version {version}"
        )
        .into());
    }
    println!("verified real OpenColorIO runtime {version}");
    Ok(())
}
