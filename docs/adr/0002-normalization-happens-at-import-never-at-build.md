# Containers are normalized at import, never by the overlay builder

Normalizing a mod's container (rewriting deflated packed-WAD entries as Stored so they can be
read seekably in place, correcting archive metadata) is a `ltk_fantome` library operation that an
importer - LTK Manager - runs on copies it owns. The overlay builder never mutates a source
archive: a still-deflated archive it meets falls back to the lazy in-memory mount, evicted per
WAD after its chunks are copied. We chose this over build-time rewriting (the builder is pointed
at files it does not own, such as a user's downloads) and over a temp-file spill for deflated
entries (doubles disk writes for a throwaway cache and punishes users on non-NVMe storage). The
consequence: memory on the build path is only fully bounded for normalized containers; the
in-memory fallback is the accepted cost of leaving user files untouched.
