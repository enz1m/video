# Media services

Video browses Plex, Jellyfin and Emby libraries and plays their original direct
streams, or a server transcode chosen in the player's quality menu. It supports search, paged results, artwork, movie details, show/season/
episode navigation, profiles, resume and progress reporting. The player returns
to the current listing. For a series, Next advances in server episode order and
continues into the next season; playback advances automatically at end of stream.
Autoplay waits for the complete episode, including credits and any final scenes.
Server-generated intro/credits markers are not yet shown as skip controls.
Multipart media, offline downloads and online discovery are not implemented.

The quality menu offers the original file, played directly, or a transcode
from 20 Mbps 1080p down to 720 kbps 360p, remembered across titles. A
transcode is an HLS stream of H.264 and AAC that the server encodes on demand
(Plex's universal transcoder, or the Jellyfin/Emby `master.m3u8` stream), so a
slow connection or a codec the system cannot decode still plays. It carries
the chosen audio track only and burns the chosen subtitles into the picture:
while transcoding, the audio and subtitle menus list the server's tracks, and
choosing one, or another quality, restarts the stream at the same position.
Video stops each transcode on the server when it leaves it. The original can
buffer if sustained network throughput is lower than its bitrate.

Use the library action to add a server. `--plex` opens source selection;
`--plex-server URL` opens a specified Plex address. Local discovery can find
servers on the current network. A trusted local server may allow anonymous
access; otherwise use the provider's normal account sign-in.

Plex sign-in opens Plex's account page in the default browser (or by QR code
or copied link) and continues once the request is approved. Home profiles and
protected PINs are supported. Server and profile changes retire the previous
playback and pending requests. Jellyfin and Emby use the corresponding server's
sign-in flow.

Tokens use Secret Service. A locked/unavailable keyring or cancelled storage
leaves the session unsigned-in or session-only as appropriate. Preferences and
diagnostics exclude credentials and authenticated stream URLs. Sign-out removes
saved credentials and reports failures. Video does not collect Plex passwords.

Artwork and response sizes are bounded. Credential-bearing requests remain on
the selected provider origin; redirects and cross-server resources cannot leak
tokens. HTTPS destinations follow the framework's URL and TLS policy.
