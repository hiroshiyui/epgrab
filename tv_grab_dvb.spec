# Note that this is NOT a relocatable package
Name: tv_grab_dvb
Version: 0.10
Release: 1
Summary: dump dvb epg info in xmltv
URL: http://www.darkskiez.co.uk/index.php?page=tv_grab_dvb
License: GPL
Group: Applications/Multimedia
Packager: Antonio Beamud Montero <antonio.beamud@gmail.com>
Source: %{name}.tar.gz
BuildRoot: %{_tmppath}/%{name}-%{version}-buildroot

%description
A Linux program to dump DVB EPG info in xmltv format..i.e. Extract the TV Guide from the digital TV broadcasts.

%prep
%setup -n %{name}

%build
make

%install
install -m 644 -d $RPM_BUILD_ROOT%{_bindir}/
install -m 644 -d $RPM_BUILD_ROOT%{_mandir}/man1/
install -m 644 -d $RPM_BUILD_ROOT%{_defaultdocdir}/%{name}/
install -m 755 tv_grab_dvb $RPM_BUILD_ROOT%{_bindir}/
install -m 644 tv_grab_dvb.1 $RPM_BUILD_ROOT%{_mandir}/man1/
install -m 644 README ChangeLog channels.conf chanidents $RPM_BUILD_ROOT%{_defaultdocdir}/%{name}/ 

%clean
[ "$RPM_BUILD_ROOT" != "/" ] && rm -rf $RPM_BUILD_ROOT

%files
%defattr(-,root,root)
%doc %{_mandir}/man1/*
%doc %{_defaultdocdir}/%{name}
%{_bindir}/*
