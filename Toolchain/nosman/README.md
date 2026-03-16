# Nodos Package/Module Manager (nosman)
A command-line tool that automates the process of installing, upgrading, configuring, and removing modules from a Nodos system.

`nosman` now uses the Nodos package server API for package discovery, install downloads, publish, and unpublish flows. By default it targets `http://localhost:8080`; set `NOSMAN_PACKAGE_SERVER_TOKEN` or `NODOS_STORE_ACCESS_TOKEN` for non-interactive publish/auth, or let publish trigger the package-server device sign-in flow interactively.
