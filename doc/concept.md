# DVB EPG Concepts

**Disclaimer:** This note is written according to my limited understanding about the ETSI EN 300 468 spec, there might be something incorrect. You are welcome to submit an issue to indicate any wrong place I wrote.

## EPG Elements

An EPG (Electronic Programme Guide) typically contains:

* Channel name
* Programme title
* Programme period (start time + duration)
* Programme content description

## Mapping to DVB PSI/SI Tables

These elements map to specific DVB PSI/SI tables carried in the transport stream:

| EPG Element           | DVB Table                  | PID    | Table ID |
|-----------------------|----------------------------|--------|----------|
| Channel name          | SDT (Service Description)  | 0x0011 | 0x42     |
| Service discovery     | PAT (Program Association)  | 0x0000 | 0x00     |
| Video/audio PIDs      | PMT (Program Map)          | varies | 0x02     |
| Programme title       | EIT (Event Information)    | 0x0012 | 0x4E     |
| Programme period      | EIT (Event Information)    | 0x0012 | 0x4E     |
| Programme description | EIT (Event Information)    | 0x0012 | 0x4E     |

Note: NIT (PID 0x0010) provides network-level information (network name, transport stream parameters), but in practice channel names come from SDT, not NIT.

## Transport Stream and Multiplex

A key concept is the _transport stream_ (also called _multiplex_):

* Multiple channels (services) share the same frequency as a single transport stream
* Each transport stream carries its own PAT, SDT, and EIT sections for all services within it
* Different frequencies carry different transport streams

This means:

* We **do not** need to tune per-channel -- one tune per frequency gives us data for all services on that multiplex
* We **do** need to tune to each distinct frequency to collect data from other transport streams

## Channel Scanning

Before reading EPG data, we need to discover what services exist on each frequency. This is the channel scanning process:

1. Open the DVB frontend device
2. Tune to a frequency (using parameters from a scan file, e.g. `/usr/share/dvb/dvb-t/tw-All`)
3. Read **PAT** (PID 0x0000) -- lists all services and their PMT PIDs
4. Read **SDT** (PID 0x0011) -- maps service IDs to human-readable channel names
5. Read **PMT** (per-service PID from PAT) -- provides video and audio elementary stream PIDs
6. Repeat for each frequency
7. Save the discovered channels (e.g. in zap-format `channels.conf`)

Both PAT and SDT may span multiple sections (`section_number` 0 through `last_section_number`), so all sections must be collected for a complete picture.

## EPG Data Collection

Once channels are known, the EPG collection flow is:

1. Open the DVB frontend device
2. Group channels by frequency
3. For each frequency:
   1. Tune to the frequency
   2. Open the demux device and set a section filter for PID 0x0012 (EIT)
   3. Read EIT sections: table ID 0x4E (present/following) and 0x50-0x5F (schedule) on actual TS
   4. Parse events: start time (MJD + BCD), duration (BCD), short event descriptor (tag 0x4D)
   5. Deduplicate events by (service_id, event_id) and map to channels using `service_id`
4. Output the collected programme data (e.g. as XMLTV)

## EIT Section Structure

EIT table IDs on the actual transport stream:

| Table ID    | Content                                        |
|-------------|------------------------------------------------|
| 0x4E        | Present/following events (current + next)      |
| 0x50 - 0x5F | Schedule events (up to 16 sub-tables of future programmes) |

An EIT section contains:

* 14-byte header: table_id, section_length, service_id, version, section_number, transport_stream_id, original_network_id, etc.
* Event entries (12-byte header each): event_id, start_time (5 bytes: MJD + BCD HMS), duration (3 bytes: BCD HMS), running_status, descriptors
* 4-byte CRC32

The **short event descriptor** (tag 0x4D) within each event provides:
* ISO 639 language code (3 bytes)
* Event name
* Event description text

## DVB Text Encoding

DVB text fields use a character encoding prefix byte:

| Prefix    | Encoding                                    |
|-----------|---------------------------------------------|
| 0x01-0x05 | ISO 8859 tables                             |
| 0x10      | ISO 8859-N (3-byte prefix)                  |
| 0x11      | ISO/IEC 10646 BMP (UCS-2 big-endian)        |
| 0x14      | Big5 subset / UTF-16 BE (used in Taiwan)    |
| 0x15      | UTF-8                                       |
| 0x20-0xFF | Default table (ISO 6937)                    |

Taiwan DVB-T broadcasts typically use prefix 0x14 (UTF-16 BE) for character encoding. The term "Big5 subset" means that DVB receivers must at minimum guarantee correct display of all characters included in the Big5 encoding.
