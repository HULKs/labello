pub trait LabelloApi:
    DatasetApi
    + ImportApi
    + TaskApi
    + ImageApi
    + AnnotationApi
    + ReviewApi
    + AdjudicationApi
    + OfflineApi
    + StatsApi
    + KeybindingApi
    + PrelabelApi
    + AuthApi
    + UserApi
    + crate::BuildInformationApi
{
}

impl<T> LabelloApi for T where
    T: DatasetApi
        + ImportApi
        + TaskApi
        + ImageApi
        + AnnotationApi
        + ReviewApi
        + AdjudicationApi
        + OfflineApi
        + StatsApi
        + KeybindingApi
        + PrelabelApi
        + AuthApi
        + UserApi
    + crate::BuildInformationApi
{
}
