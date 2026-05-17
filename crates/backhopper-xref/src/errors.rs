use backhopper_core::ApplicationName;
use backhopper_xref_graph::GraphError;
use backhopper_xref_reader::ReadError;
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum XrefError {
    #[error(transparent)]
    Read(#[from] ReadError),

    #[error(transparent)]
    Graph(#[from] GraphError),

    #[error("no such application: {0}")]
    NoSuchApplication(ApplicationName),

    #[error("duplicate application registered: {0}")]
    DuplicateApplication(ApplicationName),
}
