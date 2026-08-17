/// Even though different [Sources](crate::interface::source::Source) and [Targets](crate::interface::target::Target)
/// may, in theory, use different identifiers, we demand at least one of the systems to be able
/// to store the identifier of the counterpart.
/// Typically, this means the Target should store the identifier provided by the Source, either
/// as Target identifier directly, or - if that's not possible - in a custom attribute.
/// This is required because we need to be able to match entities between Source and Target,
/// and alternatives such as matching by name are not reliable enough.
///
/// Currently, we enforce no restrictions on what that identifier should look like (for example,
/// a UUID format), but this custom type is intended to signal that we are not dealing with arbitrary
/// Strings here, but identifiers of *some* kind of shared format.
///
/// Specifically, we identified two issues that lead us to demand an identifier mapping:
///
/// ## Timing attack
/// 1. A group `admin` (ID `X`) is being deleted.
/// 2. Syncer is notified of change (1)
/// 3. A group `admin` (ID `Y`) is created again.
/// 4. Syncer is notified of change (3); this notification overtakes notification of (2)
///    for some reason (a race condition occurs)
/// 5. If we match groups by ID, this is not a problem. If we match by name instead, undefined
///    behavior occurs, and there is probably no group named `admin` in the target system until the
///    next full sync.
///
/// ## Access attack
/// 1. Source group `S` is created with mapping to a target group named `T`
/// 2. Malicious source group `S-hack` is created with mapping to a target group also named `T`
/// 3. If we only match by name, members of `S-hack` would receive access to `T` belonging to `S`,
///    even though `T` belonging to `S-hack` should really be a semantically different group, just with
///    the same name. Enforcing an ID mapping is necessary in order to notice and resolve the conflict here.
pub type SharedResourceIdentifier = String;

/// A properly encoded URL path.
///
/// Use [`from_segments`](Self::from_segments) or
/// [`from_segments_and_query``](Self::from_segments_and_query) to create it.
#[derive(Debug)]
pub struct ApiPath(String);

impl ApiPath {
    /// First, [url-encodes](urlencoding::encode) each `segment` in `segments`.
    ///
    /// Then, returns a path of the form `segments[0]/segments[1]/.../segments[N-1]` for the encoded `segments`.
    pub fn from_segments<const N: usize>(segments: [&str; N]) -> Self {
        let segments = segments.into_iter().map(|segment| urlencoding::encode(segment)).collect::<Vec<_>>();
        Self(segments.join("/"))
    }
    /// First, [url-encodes](urlencoding::encode) each `segment` in `segments` and each `value` in `query_parameters[i].1`.
    ///
    /// Then, returns a path of the form `segments[0]/.../segments[N-1]?query_parameters[0].0=query_parameters[0].1&...&query_parameters[M-1].0=query_parameters[M-1].1`
    /// for the encoded `segments` and `query_parameters[i].1`.
    ///
    /// It does not encode the keys of the query parameters.
    pub fn from_segments_and_query<const N: usize, const M: usize>(segments: [&str; N], query_parameters: [(&str, &str); M]) -> Self {
        let segments = segments.into_iter().map(|segment| urlencoding::encode(segment)).collect::<Vec<_>>();
        let path_section = segments.join("/");
        let query_parameters = query_parameters
            .into_iter()
            .map(|(key, value)| format!("{key}={}", urlencoding::encode(value)))
            .collect::<Vec<_>>();
        let query_section = query_parameters.join("&");
        Self(format!("{path_section}?{query_section}"))
    }
}

impl std::fmt::Display for ApiPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}
