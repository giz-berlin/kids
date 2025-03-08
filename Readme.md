# KIDS: Keycloak Identity Syncer

A software suite to sync users actively from Keycloak to other applications user databases on a regular basis. This might be helpful if an application does not support an LDAP backend natively.

If there is a LDAP backend available, please take a look on [Keycloak LDAP server](https://rechenknecht.net/giz/keycloak/keycloak-ldap-server).

## Supported Applications

Currently, these applications are supported as sync targets:

* None, see issues

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

## Target

This component is responsible to sync the data received via the interface to the target application as well as to fetch the current state of the target application.

It has to translate the data format and ensure that the data is applied on the target.

This repository contains multiple targets, each with a single syncer app per source/target combination.

### Interface

The basic interface a target must offer is like

* `getGroups() -> Set\<UUID\>`
    * Will be used by Controller to determine which groups have to be deleted during full sync
* `getUsers() -> Set\<UUID\>`
    * Will be used by Controller to determine which users have to be deleted during full sync
* `deleteGroup(group: UUID)`
    * Will be called for child groups first
* `deleteUser(user: UUID)`
* `createOrUpdateGroup(group: Group)`
    * Only updates group attributes / authorizations or propagates these to users / other objects
    * Not intended to manage group memberships
    * Will be called for parent groups first
* `createOrUpdateUser(user: User)`
    * Update user attributes and authorization details
    * Will be called after createOrUpdateGroup
