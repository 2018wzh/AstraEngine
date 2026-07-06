use crate::{AstraContainerReader, ContainerError, ContainerKind};

#[derive(Debug, Clone)]
pub struct PackageReader {
    container: AstraContainerReader,
}

impl PackageReader {
    pub fn open(bytes: &[u8]) -> Result<Self, ContainerError> {
        let container = AstraContainerReader::new(bytes)?;
        if container.kind() != ContainerKind::Package {
            return Err(ContainerError::message("container is not a package"));
        }
        Ok(Self { container })
    }

    pub fn has_section(&self, id: &str) -> bool {
        self.container.has_section(id)
    }

    pub fn container(&self) -> &AstraContainerReader {
        &self.container
    }
}
