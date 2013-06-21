# EPGrab

Grab DVB EPG data, output to XMLTV format.

EPGrab is a fork of 'tv\_grab\_dvb' from http://bryars.eu/projects/tv\_grab\_dvb/

## Minimum Requirements

* A Linux supported DVB receiver device, see: http://www.linuxtv.org/wiki/index.php/Hardware\_Device\_Information
* Have DVB apps (http://www.linuxtv.org/wiki/index.php/LinuxTV\_dvb-apps) installed on your system.

## Build

I do use CMake build system:

* <code>cmake .</code>
* <code>make</code>

## Run

You have to use DVB apps' \*zap utilities to set up your DVB receiver. Before that, use 'scan' utility to generate a channels list file.

For instance:

<code>tzap -F -c ~/channels.conf "公共電視 PTS"</code>

Then you can run EPGrab's executable file 'epgrab' to grab DVB EPG data.
