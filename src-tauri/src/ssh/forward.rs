//! Local accept loop + `direct-tcpip` channel piping with byte counters; on 3
//! consecutive forward failures fires the per-attempt wake (never the parent,
//! F26/F27b).
//!
//! TODO(M1): accept loop, bidirectional counting copy, per-attempt fail count.
