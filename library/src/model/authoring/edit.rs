use super::{
    AuthoringProject, MediaTime, ModuleDefinitionId, ModuleInstanceId, TimelineId, TimelineItemId,
};

const DEFAULT_UNDO_LIMIT: usize = 100;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct ProjectRevision(u64);

impl ProjectRevision {
    pub const fn initial() -> Self {
        Self(0)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    fn next(self) -> Result<Self, String> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(|| "Project revision overflow".to_string())
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ProjectInvalidation {
    ProjectStructure,
    TimelineStructure {
        timeline_id: TimelineId,
    },
    TimelineRange {
        timeline_id: TimelineId,
        start: MediaTime,
        duration: MediaTime,
    },
    Item {
        timeline_id: TimelineId,
        item_id: TimelineItemId,
    },
    ModuleDefinition {
        definition_id: ModuleDefinitionId,
    },
    ModuleInstance {
        instance_id: ModuleInstanceId,
    },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChangeSet {
    pub revision: ProjectRevision,
    pub invalidations: Vec<ProjectInvalidation>,
}

/// Non-persisted editing state. Every mutation is validated on an isolated
/// candidate before replacing the authoritative Project.
pub struct AuthoringSession {
    project: AuthoringProject,
    revision: ProjectRevision,
    undo: Vec<AuthoringProject>,
    redo: Vec<AuthoringProject>,
    undo_limit: usize,
}

impl AuthoringSession {
    pub fn new(project: AuthoringProject) -> Result<Self, String> {
        Self::with_undo_limit(project, DEFAULT_UNDO_LIMIT)
    }

    pub fn with_undo_limit(project: AuthoringProject, undo_limit: usize) -> Result<Self, String> {
        project.validate()?;
        Ok(Self {
            project,
            revision: ProjectRevision::initial(),
            undo: Vec::new(),
            redo: Vec::new(),
            undo_limit,
        })
    }

    pub fn project(&self) -> &AuthoringProject {
        &self.project
    }

    pub fn revision(&self) -> ProjectRevision {
        self.revision
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn into_project(self) -> AuthoringProject {
        self.project
    }

    pub(crate) fn transact<T>(
        &mut self,
        invalidations: Vec<ProjectInvalidation>,
        edit: impl FnOnce(&mut AuthoringProject) -> Result<T, String>,
    ) -> Result<(T, ChangeSet), String> {
        let next_revision = self.revision.next()?;
        let mut candidate = self.project.clone();
        let result = edit(&mut candidate)?;
        candidate.validate()?;

        let previous = std::mem::replace(&mut self.project, candidate);
        if self.undo_limit > 0 {
            if self.undo.len() == self.undo_limit {
                self.undo.remove(0);
            }
            self.undo.push(previous);
        }
        self.redo.clear();
        self.revision = next_revision;
        Ok((
            result,
            ChangeSet {
                revision: self.revision,
                invalidations,
            },
        ))
    }

    pub fn undo(&mut self) -> Result<Option<ChangeSet>, String> {
        let Some(previous) = self.undo.pop() else {
            return Ok(None);
        };
        let next_revision = self.revision.next()?;
        previous.validate()?;
        self.redo
            .push(std::mem::replace(&mut self.project, previous));
        self.revision = next_revision;
        Ok(Some(ChangeSet {
            revision: self.revision,
            invalidations: vec![ProjectInvalidation::ProjectStructure],
        }))
    }

    pub fn redo(&mut self) -> Result<Option<ChangeSet>, String> {
        let Some(next) = self.redo.pop() else {
            return Ok(None);
        };
        let next_revision = self.revision.next()?;
        next.validate()?;
        let previous = std::mem::replace(&mut self.project, next);
        if self.undo_limit > 0 {
            if self.undo.len() == self.undo_limit {
                self.undo.remove(0);
            }
            self.undo.push(previous);
        }
        self.revision = next_revision;
        Ok(Some(ChangeSet {
            revision: self.revision,
            invalidations: vec![ProjectInvalidation::ProjectStructure],
        }))
    }
}
