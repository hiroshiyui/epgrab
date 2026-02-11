# References

## Linux DVB API

* [Linux Media Subsystem Documentation](https://www.kernel.org/doc/html/latest/media/index.html) -- kernel documentation for V4L2 and DVB subsystems
* [DVB Frontend API](https://www.kernel.org/doc/html/latest/media/dvb/frontend.html) -- frontend device ioctls (`FE_SET_PROPERTY`, `FE_READ_STATUS`, etc.)
* [DVB Demux API](https://www.kernel.org/doc/html/latest/media/dvb/demux.html) -- demux device ioctls (`DMX_SET_FILTER`, section filtering)

## DVB Specifications

* [ETSI EN 300 468 (PDF) - Specification for Service Information (SI) in DVB systems](https://www.etsi.org/deliver/etsi_en/300400_300499/300468/01.15.01_60/en_300468v011501p.pdf) -- the primary spec for DVB SI tables (PAT, SDT, EIT, NIT, PMT, etc.)
* [ETSI EN 300 744 - DVB-T framing structure](https://www.etsi.org/deliver/etsi_en/300700_300799/300744/01.06.02_60/en_300744v010602p.pdf) -- DVB-T physical layer (modulation, guard intervals, etc.)
* [ISO/IEC 13818-1 (MPEG-2 Systems)](https://www.iso.org/standard/74427.html) -- MPEG transport stream structure, PAT/PMT table definitions

## Taiwan DVB-T

* [NCC: 地面數位電視接收機基本技術規範](https://www.ncc.gov.tw/) -- Taiwan terrestrial digital TV receiver technical specifications
* Scan file: `/usr/share/dvb/dvb-t/tw-All` (provided by the `dtv-scan-tables` package)

## XMLTV

* [XMLTV File Format](http://wiki.xmltv.org/index.php/XMLTVFormat) -- standard XML format for exchanging TV programme listings

## Tools and Community

* [linuxtv.org Wiki](https://www.linuxtv.org/wiki/index.php/Main_Page) -- community wiki for Linux DVB/V4L development
* [v4l-utils](https://git.linuxtv.org/v4l-utils.git) -- userspace utilities for V4L/DVB devices (`dvbv5-zap`, `dvbv5-scan`, etc.)
* [dtv-scan-tables](https://git.linuxtv.org/dtv-scan-tables.git) -- DVB channel scan data for various countries/regions

## Related Implementations

* [Kaffeine](https://kde.org/applications/multimedia/org.kde.kaffeine) -- open source DVB player with channel scanning and EPG browsing
* [VLC media player](https://www.videolan.org/vlc/) -- open source media player with DVB playback support
* [libdvbpsi](https://www.videolan.org/developers/libdvbpsi.html) -- C library for DVB PSI table decoding, used by VLC
* [tv_grab_dvb](https://sourceforge.net/projects/tv-grab-dvb/) -- a DVB EIT to XMLTV grabber
