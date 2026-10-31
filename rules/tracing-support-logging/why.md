Calls to log::*! or raw tracing::event! bypass the tracing_support event sink, so logs and error reports silently disappear.
