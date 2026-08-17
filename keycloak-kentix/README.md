# Kentix Target

Synchronizes source users to Kentix AccessManager users and their access profiles.
It was tested using version 7.1.2.

## Working principles

This adapter syncs:

1. Keycloak users --> Kentix AccessManager users, mapping
    * UUID (Keycloak sub) --> Kentix AccessManager username
    * User name --> Full name in Kentix AccessManager
    * E-Mail
    * Enabled state
    * `rfid_uid_attribute_name` configured user attribute --> UID attribute in Kentix AccessManager (users are skipped or deleted if this attribute is empty)
    * `rfid_data_attribute_name` configure user attribute --> UID data attribute in Kentix AccessManager
1. Client roles of the configured Keycloak client (except special roles, see [config](../default_configs/kentix-target.config.example.toml)) --> Access profiles (profile must exist beforehand, matched by name)
1. Users with client role `offline_access_role` --> Emergency access to the doorlock

## Configuration

Please refer to the [configuration example file](../default_configs/kentix-target.config.example.toml).

## CA

You will need a custom certificate for Kentix that has a CA (not just a bare token) and common name, organization, and organizational unit filled in.
Pass the CA certificate as `kentix_root_certificate_pem_path` to KIDS and add a sub-certificate (also with CN, O, and OU) to Kentix via its Web UI.
Certificates using RSA are verified to be working, elliptic curves pose problems with Kentix.
