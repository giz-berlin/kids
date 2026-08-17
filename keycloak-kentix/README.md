# Kentix Target

Synchronizes source users to Kentix users and their access profiles.

## Working principles

We sync source users to Kentix users.
We use the source user's id as the Kentix user's username and the source user's username as the Kentix user's full name.
All source roles assigned to a source user must match an access profile by name.
Users with that role will be assigned to the matching Kentix access profile.

## Configuration

Please refer to the [configuration example file](../default_configs/kentix-target.config.example.toml).

## CA

You will need a custom certificate for Kentix that has a CA (not just a bare token) and common name, organization, and organizational unit filled in.
Pass the CA certificate as `kentix_root_certificate_pem_path` to KIDS and add a sub-certificate (also with CN, O, and OU) to Kentix via its Web UI.
Certificates using RSA are verified to be working, elliptic curves pose problems with Kentix.
