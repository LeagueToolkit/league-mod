# Pass-through recomputes chunk checksums and warns on mismatch, never fails

An overlay build that passes a chunk's compressed bytes through from a mod's container always
recomputes the xxh3 checksum over those bytes in flight and writes the computed value to the
overlay TOC; the source's claimed checksum is only a hint. A mismatch between claimed and
computed is reported as structured data in the build result (and a warning log), and the build
succeeds. We chose this over failing the build (fantome tools in the wild ship wrong metadata
over perfectly good bytes - the same tools already ship wrong zip CRCs, which we also tolerate)
and over trusting the source blindly (the client kills the process with "Installation is
corrupt" on any checksum that does not match its bytes, which is the worst possible place to
surface a lying TOC). Recomputation runs at roughly memcpy speed, so correctness costs nothing
measurable.
