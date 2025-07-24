# KIDS: Keycloak Identity Syncer

A software suite to sync users actively from Keycloak to other applications user databases on a regular basis. This might be helpful if an application does not support an LDAP backend natively.

If there is a LDAP backend available, please take a look on [Keycloak LDAP server](https://rechenknecht.net/giz/keycloak/keycloak-ldap-server).

## Supported Applications

Currently, these applications are supported as sync targets:

* None, see issues

## Configuration

We use TOML for our configuration. A config file should be assembled by concatenating the 
[general configuration options applying to all use cases](default_configs/config.example.toml) as well as the configuration
required for the used `Source` and `Target` components (see [architecture](#architecture)).

All default configurations can be found in the [default config folder](default_configs).

## Architecture

This project consists of three main components:

1. Source (Keycloak)
2. Controller
3. Target (Application)

For just in time sync, there is a fourth component: KIDS event listener. This is a Keycloak event listener issuing web requests to syncer instances if certain objects are modified. This component is only necessary if a near real-time sync is desired.

### Source

The source (currently only Keycloak) provides the user data. It has the responsibility to provide a common interface to interact with the data source.

In the Keycloak case, we use the Keycloak Admin API to fetch the data transparently.

### Controller

The controller is responsible to schedule syncs and to receive and process events from the KIDS event listener. It is the central component using the defined interfaces of the [target](#target) and [source](#source).

### Target

This component is responsible to sync the data received via the interface to the target application as well as to fetch the current state of the target application.

It has to translate the data format and ensure that the data is applied on the target.

This repository contains multiple targets, each with a single syncer app per source/target combination.

## Manual testing

For manual end-to-end testing see [here](e2e-test/README.md).

## Debugging

### Backtraces

For easier error handling we use the anyhow create. The Error type can also captures and print backtraces if an error occurred. To see the backtrace you have to enable it through environment variables. You have the following options:

- `RUST_BACKTRACE=1`: backtraces for panics and errors
- `RUST_LIB_BACKTRACE=1`: backtraces just for errors
- `RUST_BACKTRACE=1` and `RUST_LIB_BACKTRACE=0`: backtraces just for panics
