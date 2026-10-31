use log::trace;

impl RelayClient {
    fn prepare_request(&self, method: Method, url: Url) -> Result<Request> {
        trace!("making {} request", method.as_str()); // flagged: log crate macro
        // …
    }
}
