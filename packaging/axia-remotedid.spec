# Distribution package for the already compiled Fedora alpha runtime.
%global debug_package %{nil}
%global _build_id_links none

Name:           axia-remotedid
Version:        0.1.0
Release:        0.4.alpha4%{?dist}
Summary:        Cloud digital certificates for Linux
License:        Apache-2.0
URL:            https://github.com/rodolfoaltenrath/axia-remoteid
Source0:        axia-remotedid-runtime.tar.gz
ExclusiveArch:  x86_64
Prefix:         /usr
BuildRequires:  desktop-file-utils
BuildRequires:  openssl-libs
BuildRequires:  gtk4
Requires:       gtk4
Requires:       nss
Requires:       ca-certificates
Requires:       fedora-release >= 44
Requires:       fedora-release < 45
Recommends:     (gnome-shell-extension-appindicator if gnome-shell)

%description
Axia RemotedID connects Certisign cloud certificates to compatible Linux
applications. This alpha provides graphical account connection, certificate
authorization and discovery of installed native Firefox/Chromium browsers.
All application operations are compiled Rust and invoke no Python or Zenity.
Each user connects their own
account. Browser setup runs in the user's session, never during RPM installation.

%prep
%setup -q -n axia-remotedid-runtime

%build
# Runtime binaries were compiled and tested by scripts/build_fedora.sh.

%install
install -Dpm0755 desktopid-client %{buildroot}%{_libexecdir}/%{name}/desktopid-client
install -Dpm0755 libdesktopid_pkcs11.so %{buildroot}%{_libdir}/%{name}/libdesktopid_pkcs11.so
mkdir -p %{buildroot}%{_bindir}
ln %{buildroot}%{_libexecdir}/%{name}/desktopid-client %{buildroot}%{_bindir}/axia-remotedid
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
%{_datadir}/applications/axia-remotedid.desktop

%changelog
* Fri Oct 16 2026 Axia RemotedID contributors - 0.1.0-0.4.alpha4
- Simplify certificate authorization and move privacy details to About.
- Add native About dialog with version, license and project link.
- Wait for laboratory process readiness before testing Chromium detection.
* Thu Oct 15 2026 Axia RemotedID contributors - 0.1.0-0.3.alpha3
- Exclude uninitialized Firefox and LibreWolf profiles from configuration.
- Explain profile selection and preserve initialized secondary profiles.
- Extend NSS regressions and stabilize parallel tray lock test.
* Thu Oct 15 2026 Axia RemotedID contributors - 0.1.0-0.2.alpha2
- Publish native alpha.2 with runtime guards and account recovery.
- Include secondary Firefox profiles, LibreWolf discovery and per-profile results.
* Wed Oct 14 2026 Axia RemotedID contributors - 0.1.0-0.2.alpha2
- Compile desktop, browser setup, installation and authorization UI in Rust.
- Detect installed browsers without a direct Python, Zenity or browser requirement.
* Tue Oct 13 2026 Axia RemotedID contributors - 0.1.0-0.1.alpha1
- Initial Fedora 44 alpha with desktop application and user-session Firefox setup.
