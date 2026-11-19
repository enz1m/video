# Media formats

GStreamer supplies decoding, playback, seeking and track discovery. Actual codec
support depends on installed runtime plugins and the media's encoding; a file
extension alone cannot guarantee playback.

Folder discovery and Linux “Open With” cover Matroska (`mkv`), MP4/M4V, WebM,
QuickTime (`mov`), AVI, WMV, FLV, Ogg video (`ogv`), MPEG program streams
(`mpg`, `mpeg`, `vob`), MPEG transport streams (`ts`, `mts`, `m2ts`), and
3GPP/3GPP2 (`3gp`, `3g2`). MPEG `.ts` shares an extension with text formats; the
desktop uses file contents to distinguish them. Common MIME aliases
are included for older desktop databases.

Individual local files are probed regardless of their extension. Native file URIs
preserve literal filenames and non-UTF-8 Unix paths. Folders queue recognized
video files. HTTP/HTTPS streams follow the framework URL policy. Unsafe URLs
and unsupported media fail visibly without replacing valid playback state.

External subtitle discovery supports SRT, ASS/SSA, WebVTT and SUB filenames.
Codec and subtitle capabilities still depend on GStreamer plugins. Exact sibling
stems take priority over language/forced suffixes. Audio/subtitle menus reflect
tracks reported by the media backend. Text subtitles have six sizes, from Tiny
to Extra large, and timing offsets. Text captions reserve space above the playback
controls, including in compact windows. Plain-text cues discard stray ASS positioning
commands; native ASS/SSA streams retain their renderer's styling. Bitmap subtitle
size and authored ASS styles may be controlled by the subtitle format itself.

Bad, cancelled or superseded opens cannot install a retired source. Native
transfers keep stable local references in place and retain temporary exported
media before a drop completes. Recent snapshots and thumbnails are bounded;
exported frame names are confined and never overwrite existing files.
