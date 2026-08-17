# Kentix Target

Synchronizes source users to Kentix users and their access profiles.

## Working principles

We sync source users to Kentix users.
We use the source user's id as the Kentix user's username and the source user's username as the Kentix user's full name.
All source roles assigned to a source user must match an access profile by name.
Users with that role will be assigned to the matching Kentix access profile.

## Configuration

Please refer to the [configuration example file](../default_configs/kentix-target.config.example.toml).
