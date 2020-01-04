**Disclaimer:** This note is written according to my limited understanding about the ETSI EN 300 468 spec, there might be something incorrect. You are welcome to submit an issue to indicate any wrong place I wrote.

As an EPG (electronic program guide), it usually has these elements:

* Channel name
* Program title
* Program period (start -> stop)
* Program content description

And we would like to map these elements to the structure in DVB PSI/SI packet/table:

* Channel name: PSI -> NIT (0x0010) -> network_information_section -> network_name_descriptor
* Program title: PSI -> EIT (0x0012)
* Program period: PSI -> EIT (0x0012)
* Program content description: PSI -> EIT (0x0012)

Additionally, we should understand the _network_ concept:

* Each channel is belongs to a network
* Each network may contains multiple channels
* Each network is allocated to a specified frequency

While developing a DVB EPG decoder, we should remember:

* We don't have to tune to each _channel_ to fetch its own EIT data if these channels are in _the same network_
* On the contrary, We have to tune to different _network_ to fetch NIT & EIT data of the other networks

So, practically, we could programming the code as:

1. Open demuxer device
1. Let demuxer tune (or _zap_) to the frequency of a network (such as what `dvbv5-zap -c ~/dvb_channel.conf -m -v 533000000` does for instance)
1. Fetch NIT data
1. Fetch EIT data
1. Open, build the XMLTV document, then close it
1. Close demuxer device