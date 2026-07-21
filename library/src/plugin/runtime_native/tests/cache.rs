    #[test]
    fn runtime_cache_keys_include_plugin_operation_source_config_and_time() {
        let image_a = LoadRequest::Image {
            path: "/virtual/a.fixture".to_string(),
        };
        let image_b = LoadRequest::Image {
            path: "/virtual/b.fixture".to_string(),
        };
        assert_ne!(
            runtime_loader_cache_key("loader.a", &image_a),
            runtime_loader_cache_key("loader.b", &image_a)
        );
        assert_ne!(
            runtime_loader_cache_key("loader.a", &image_a),
            runtime_loader_cache_key("loader.a", &image_b)
        );
        assert_ne!(source_time_bits(0.25), source_time_bits(0.5));

        let first = EffectConfigKey(vec![(
            "amount".to_string(),
            PropertyValue::Number(OrderedFloat(0.25)),
        )]);
        let second = EffectConfigKey(vec![(
            "amount".to_string(),
            PropertyValue::Number(OrderedFloat(0.5)),
        )]);
        assert_ne!(first, second);
    }

    #[cfg(unix)]
    #[test]
    fn runtime_loader_cache_detects_same_size_mtime_file_replacement()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory =
            std::env::temp_dir().join(format!("runtime-loader-identity-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory)?;
        let path = directory.join("source.rgba-fixture");
        let replacement = directory.join("replacement.rgba-fixture");
        std::fs::write(&path, b"aaaa")?;
        let original_metadata = std::fs::metadata(&path)?;
        let original_modified = original_metadata.modified()?;
        let request = LoadRequest::Image {
            path: path.to_string_lossy().into_owned(),
        };
        let original_key = runtime_loader_cache_key("loader.identity", &request);

        std::fs::write(&replacement, b"bbbb")?;
        let replacement_file = std::fs::OpenOptions::new().write(true).open(&replacement)?;
        replacement_file.set_times(std::fs::FileTimes::new().set_modified(original_modified))?;
        std::fs::rename(&replacement, &path)?;

        let replaced_metadata = std::fs::metadata(&path)?;
        assert_eq!(replaced_metadata.len(), original_metadata.len());
        assert_eq!(replaced_metadata.modified()?, original_modified);
        let replaced_key = runtime_loader_cache_key("loader.identity", &request);
        assert_ne!(
            original_key, replaced_key,
            "device/inode/ctime identity must invalidate a same-path, same-size, same-mtime replacement"
        );

        std::fs::remove_file(path)?;
        std::fs::remove_dir(directory)?;
        Ok(())
    }
