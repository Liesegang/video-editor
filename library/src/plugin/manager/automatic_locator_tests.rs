use super::*;
use crate::model::frame::Image;
use crate::plugin::{LoadPluginResult, Plugin};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

struct CountingLoader {
    calls: Arc<AtomicUsize>,
    connect_to: Option<SocketAddr>,
    response: bool,
}

impl Plugin for CountingLoader {
    fn id(&self) -> &str {
        "automatic-locator-counting-loader"
    }

    fn name(&self) -> String {
        "Automatic Locator Counting Loader".to_string()
    }

    fn category(&self) -> String {
        "Tests".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl LoadPlugin for CountingLoader {
    fn open(&self, _path: &str) -> LoadPluginResult<Vec<AssetMetadata>> {
        Err(LoadPluginError::Unsupported)
    }

    fn load(
        &self,
        _request: &LoadRequest,
        _cache: &CacheManager,
    ) -> LoadPluginResult<LoadResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(address) = self.connect_to {
            let _connection = TcpStream::connect_timeout(&address, Duration::from_millis(250));
        }
        if self.response {
            return LoadResponse::abi_v1_srgb_rgba8(Image::new(1, 1, vec![1, 2, 3, 255]))
                .map_err(|error| LoadPluginError::Failed(LibraryError::Plugin(error.to_string())));
        }
        Err(LoadPluginError::Unsupported)
    }
}

fn manager_with_loader(
    calls: Arc<AtomicUsize>,
    connect_to: Option<SocketAddr>,
    response: bool,
) -> PluginManager {
    let manager = PluginManager::new();
    manager.register_load_plugin(Arc::new(CountingLoader {
        calls,
        connect_to,
        response,
    }));
    manager
}

fn image_request(path: impl AsRef<std::path::Path>) -> LoadRequest {
    LoadRequest::Image {
        path: path.as_ref().to_string_lossy().into_owned(),
    }
}

#[test]
fn automatic_http_locator_connects_zero_times_and_dispatches_zero_plugins()
-> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let manager = manager_with_loader(Arc::clone(&calls), Some(address), false);
    let request = LoadRequest::Image {
        path: format!("http://{address}/document-controlled.png"),
    };

    let error = manager
        .load_resource(&request, &CacheManager::new())
        .err()
        .ok_or_else(|| std::io::Error::other("HTTP locator was unexpectedly loaded"))?;

    assert!(error.to_string().contains("direct local regular file"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let accept = listener.accept();
    assert!(accept.is_err_and(|error| error.kind() == std::io::ErrorKind::WouldBlock));
    Ok(())
}

#[cfg(unix)]
#[test]
fn automatic_fifo_is_rejected_before_plugin_dispatch_without_blocking()
-> Result<(), Box<dyn std::error::Error>> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let directory = tempfile::tempdir()?;
    let fifo = directory.path().join("document-controlled.png");
    let fifo_path = CString::new(fifo.as_os_str().as_bytes())?;
    // SAFETY: `fifo_path` is a live NUL-terminated path and `mkfifo` does not
    // retain the pointer after returning.
    let status = unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) };
    if status != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let manager = manager_with_loader(Arc::clone(&calls), None, false);

    let error = manager
        .load_resource(&image_request(&fifo), &CacheManager::new())
        .err()
        .ok_or_else(|| std::io::Error::other("FIFO was unexpectedly loaded"))?;

    assert!(error.to_string().contains("FIFOs"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[cfg(unix)]
#[test]
fn automatic_symlink_to_device_is_rejected_before_plugin_dispatch()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir()?;
    let link = directory.path().join("document-controlled.png");
    symlink("/dev/null", &link)?;
    let calls = Arc::new(AtomicUsize::new(0));
    let manager = manager_with_loader(Arc::clone(&calls), None, false);

    let error = manager
        .load_resource(&image_request(&link), &CacheManager::new())
        .err()
        .ok_or_else(|| std::io::Error::other("device symlink was unexpectedly loaded"))?;

    assert!(error.to_string().contains("symbolic links"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn automatic_regular_local_fixture_reaches_loader_and_succeeds()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let fixture = directory.path().join("fixture.custom");
    std::fs::write(&fixture, b"regular local fixture")?;
    let calls = Arc::new(AtomicUsize::new(0));
    let manager = manager_with_loader(Arc::clone(&calls), None, true);

    let image = manager
        .load_resource(&image_request(&fixture), &CacheManager::new())?
        .into_rgba8()?;

    assert_eq!((image.width, image.height), (1, 1));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}
