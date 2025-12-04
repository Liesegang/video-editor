use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;
use library::model::project::project::Project;
use log::{debug, error, info};
use qobject::TrackRoles;
use std::fs::File;
use std::io::Read;
use std::pin::Pin;

#[cxx_qt::bridge]
mod qobject {
    unsafe extern "C++" {
        include!(< QAbstractListModel >);
        type QAbstractListModel;

        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
    }

    #[qenum(TrackList)]
    enum TrackRoles {
        Name,
        Height,
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        type TrackList = super::TrackListRust;

        #[cxx_override]
        #[rust_name = "row_count"]
        fn rowCount(self: &TrackList, parent: &QModelIndex) -> i32;

        #[cxx_override]
        fn data(self: &TrackList, index: &QModelIndex, role: i32) -> QVariant;

        #[cxx_override]
        #[rust_name = "role_names"]
        fn roleNames(self: &TrackList) -> QHash_i32_QByteArray;
    }

    unsafe extern "RustQt" {
        #[qinvokable]
        #[rust_name = "load_tracks"]
        fn loadTracks(self: Pin<&mut TrackList>);

        #[inherit]
        #[rust_name = "begin_reset_model"]
        fn beginResetModel(self: Pin<&mut TrackList>);

        #[inherit]
        #[rust_name = "end_reset_model"]
        fn endResetModel(self: Pin<&mut TrackList>);
    }
}

pub struct TrackListRust {
    tracks: Vec<(QString, u32)>,
}

impl Default for TrackListRust {
    fn default() -> Self {
        Self {
            tracks: vec![
                ("Video".into(), 50),
                ("Overlay".into(), 40),
                ("Effect".into(), 35),
            ],
        }
    }
}

use qobject::*;

impl qobject::TrackList {
    fn row_count(&self, _parent: &QModelIndex) -> i32 {
        self.tracks.len() as i32
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let role = TrackRoles { repr: role };

        if let Some((name, height)) = self.tracks.get(index.row() as usize) {
            match role {
                TrackRoles::Name => return name.into(),
                TrackRoles::Height => return height.into(),
                _ => return QVariant::default(),
            }
        } else {
            QVariant::default()
        }
    }

    fn role_names(&self) -> QHash_i32_QByteArray {
        let mut hash = QHash_i32_QByteArray::default();
        hash.insert(TrackRoles::Name.repr, "name".into());
        hash.insert(TrackRoles::Height.repr, "height".into());
        hash
    }

    fn load_tracks(mut self: Pin<&mut Self>) {
        let file = File::open("project.json");
        if let Err(e) = file {
            error!("Error opening file: {}", e);
            return;
        }
        let mut file = file.unwrap();

        let mut project_string = String::new();
        if let Err(e) = file.read_to_string(&mut project_string) {
            error!("Error reading file: {}", e);
            return;
        }

        let project = Project::load(&project_string);
        if let Err(e) = project {
            error!("Error loading project: {}", e);
            return;
        }
        let project = project.unwrap();

        let tracks = project.compositions[0].tracks.clone();
        self.as_mut().begin_reset_model();
        let new_tracks: Vec<(QString, u32)> = tracks
            .into_iter()
            .map(|track| (track.name.into(), 30))
            .collect();
        info!("Loaded {} tracks from project.json", new_tracks.len());
        self.as_mut().rust_mut().tracks = new_tracks;
        self.as_mut().end_reset_model();
        debug!("Track list model reset complete");
    }
}
