# KIDS: Keycloak Identity Syncer

A software suite to sync users actively from Keycloak to other applications user databases on a regular basis. This might be helpful if an application does not support an LDAP backend natively.

If there is a LDAP backend available, please take a look on [Keycloak LDAP server](https://rechenknecht.net/giz/keycloak/keycloak-ldap-server).

## Supported Applications

Currently, these applications are supported as sync targets:

* [James email server](https://github.com/apache/james-project)
* [Synapse matrix server](https://github.com/element-hq/synapse)

## Configuration

We use TOML for our configuration. A config file should be assembled by concatenating the
[general configuration options applying to all use cases](default_configs/config.example.toml) as well as the configuration
required for the used `Source` and `Target` components (see [architecture](#architecture)).

All default configurations can be found in the [default config folder](default_configs).

### TLS

To serve the API over HTTPS configure the `[controller.tls]` section in the config file.
The certificate and key files are inlined in PEM format and can be generated like this:

```shell
./scripts/generate-cert.sh server kids 127.0.0.1
```

Modern TLS clients verify the server certificate against its `subjectAltName` (SAN) entries so make sure to include every IP clients will actually connect through.

### mTLS

To require client certificates for authentication set the `[controller.tls.client_auth]` section with one or more pinned `[[clients]]`.
Generate the certificate and key using the following command and make sure to store the private key somewhere accessible to the client.

```shell
./scripts/generate-cert.sh client keycloak
```

## Architecture

This project consists of three main components:

1. Source (Keycloak)
2. Controller
3. Target (Application)

For just in time sync, there is a fourth component: KIDS event listener. This is a Keycloak event listener issuing web requests to syncer instances if certain objects are modified. This component is only necessary if a near real-time sync is desired.

### Sync approaches

The project aims to support two different sync types.

The syncer should ensure, that no parallel syncs are happening on the same object. However, targets should ensure that, even if this happens, no data loss occurs. Targets should also ensure that, on the next sync, a clean state can be reached.

#### Full sync

The syncer will always support a full sync of all users and groups of the source to the target.

This type is intended to be triggered e.g. once a day. During this sync, all caches are emptied and afterwards a clean state should be reached between source and target.

During full sync, targets will get triggered like with an incremental sync, but when it starts, targets will get notified to e.g. clean runtime caches.

#### Incremental sync

By utilizing the Keycloak event listener, the syncer might be notified about changes of specific groups and users. The controller will then trigger the sync to the target accordingly.

As the events are on a fire and forget basis, some changes might not be reflected in (near) real time. Therefore, to be sure, a periodic full sync is still required.

#### Group hierarchy

When a parent group is deleted in Keycloak, we don't get webhook events for all subgroup and its former subgroup relationships are also no longer accessible.
We therefore cannot guarantee that subgroups are deleted before their parent group in the target.
Targets that implement group hierarchies must handle cascading deletes internally.
Any remaining subgroups will be cleaned up on the next full sync.

#### Group updates and user updates

When a group receives an update no user update gets triggered automatically.
In case your target needs such an update the group exposes a method of fetching all its members.
This method is quite expensive when getting all users of all subgroups as well (two requests to Keycloak per group).

### Components

#### Source

The source (currently only Keycloak) provides the user data. It has the responsibility to provide a common interface to interact with the data source.

In the Keycloak case, we use the Keycloak Admin API to fetch the data transparently.

#### Controller

The controller is responsible to schedule syncs and to receive and process events from the KIDS event listener. It is the central component using the defined interfaces of the [target](#target) and [source](#source).

#### Target

This component is responsible to sync the data received via the interface to the target application as well as to fetch the current state of the target application.

It has to translate the data format and ensure that the data is applied on the target.

This repository contains multiple targets, each with a single syncer app per source/target combination.

## Manual testing

For manual end-to-end testing see [here](e2e-test/README.md).

## Debugging

### Backtraces

For easier error handling we use the anyhow create. The Error type can also capture and print backtraces if an error occurred. To see the backtrace you have to enable it through environment variables. You have the following options:

- `RUST_BACKTRACE=1`: backtraces for panics and errors
- `RUST_LIB_BACKTRACE=1`: backtraces just for errors
- `RUST_BACKTRACE=1` and `RUST_LIB_BACKTRACE=0`: backtraces just for panics
