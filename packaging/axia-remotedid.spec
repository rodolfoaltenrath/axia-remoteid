# Distribution package for the already compiled Fedora alpha runtime.
%global debug_package %{nil}
%global _build_id_links none

Name:           axia-remotedid
Version:        0.1.0
Release:        0.1.alpha1%{?dist}
Summary:        Cloud digital certificates for Linux
License:        Apache-2.0
URL:            https://github.com/rodolfoaltenrath/axia-remoteid
Source0:        axia-remotedid-runtime.tar.gz
ExclusiveArch:  x86_64
Prefix:         /usr
BuildRequires:  desktop-file-utils
BuildRequires:  openssl-libs
Requires:       python3 >= 3.11
Requires:       coreutils
Requires:       zenity
Requires:       firefox
Requires:       nss
Requires:       ca-certificates
Requires:       fedora-release >= 44
Requires:       fedora-release < 45

%description
Axia RemotedID connects Certisign cloud certificates to compatible Linux
applications. This alpha provides graphical account connection, certificate
authorization and native Firefox integration. Each user connects their own
account. Browser setup runs in the user's session, never during RPM installation.

%prep
%setup -q -n axia-remotedid-runtime

%build
# Runtime binaries were compiled and tested by scripts/build_fedora.sh.

%install
install -Dpm0755 desktopid-client %{buildroot}%{_libexecdir}/%{name}/desktopid-client
install -Dpm0755 libdesktopid_pkcs11.so %{buildroot}%{_libdir}/%{name}/libdesktopid_pkcs11.so
install -Dpm0755 axia-remotedid %{buildroot}%{_bindir}/axia-remotedid
install -Dpm0644 desktop_app.py %{buildroot}%{_datadir}/%{name}/desktop_app.py
install -Dpm0644 firefox_pkcs11.py %{buildroot}%{_datadir}/%{name}/firefox_pkcs11.py
install -Dpm0644 axia-remotedid-alpha.desktop %{buildroot}%{_datadir}/applications/axia-remotedid.desktop

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/axia-remotedid.desktop
%{buildroot}%{_libexecdir}/%{name}/desktopid-client --version

%files
%license LICENSE NOTICE
%doc README.md docs CONTINUIDADE_IA_FEDORA.md
%{_bindir}/axia-remotedid
%dir %{_libexecdir}/%{name}
%{_libexecdir}/%{name}/desktopid-client
%dir %{_libdir}/%{name}
%{_libdir}/%{name}/libdesktopid_pkcs11.so
%{_datadir}/%{name}/
%{_datadir}/applications/axia-remotedid.desktop

%changelog
* Tue Oct 13 2026 Axia RemotedID contributors - 0.1.0-0.1.alpha1
- Initial Fedora 44 alpha with desktop application and user-session Firefox setup.
