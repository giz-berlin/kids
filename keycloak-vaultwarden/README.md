# Vaultwarden Target

Synchronizes source groups and users to the groups, collections and members of a Vaultwarden organization.

## Getting Started

To configure the Vaultwarden target, take a look at [the example config](../default_configs/vaultwarden-target.config.example.toml).

Your Vaultwarden deployment needs to have an `admin_token` configured for access to the application-wide admin panel. Even if the Web-UI doesn't show the login screen because access is derived from OIDC roles, the token is still usable for accessing the admin API.

You should then create a syncer-specific user which has a master password set, and create the organization you want to sync into from this user account. They will be the owner and able to add new members, create new groups etc.

The user credentials should be copied from this user's settings, the organization credentials from the organization's settings.

## Working Principle

### Group Syncing

For each group in the source directory, we create a _Group_ and a _Collection_ belonging to that group in the organization. Only members of the group are able to create, see, modify and delete items from the respective _Collection_.

If a sync fails and only the group or the collection exists, since this is not an atomic operation, we fix this error state by deleting the remaining group or collection. If the error occured during the creation, this is not an issue as no elements should be in the collection yet. If the error occured during the deletion, this is not an issue, since we expected it to be fully removed.

### User Syncing

For each user, we first check whether they should have access to Vaultwarden. This is decided by checking if (1) the user is enabled and (2) if the user has the required role, if such a role is configured.

Based on the desired state, we either ensure that the user is `active` or `inactive`.
For `active` users, we import them and, if they already have an account, immediately confirm their membership in the organization. That allows access to the elements in it.
For `inactive` users, we disable them in the entire Vaultwarden instance and revoke their organization membership. This is later re-activated if the user ever becomes active again.

### Group Membership

Users are added to a group based on group membership in the source. Every member of a group gets full access to read, create, modify and delete elements in the group's collection.

### Email Address Stability

Emails can't be changed easily in Vaultwarden, since they are used as a salt for the cryptographic keys of a user. Because of this, we expect the email in the source directory to be stable, i.e. never change. If it ever does, KIDS will throw an error.

You can use the config option `email_attribute` to point to a user attribute with a stable email address, if this can't be ensured for the normal email field.

### Confirm after Organization Add

If a user does not have an account yet and first registers, even if they are already invited to the organization, they will only be confirmed either on the next user update (e.g. when the user is added to a group) or on the next full sync.

Since we expect the creation of a Vaultwarden account to be part of the onboarding process in the future and we expect users to be added to some groups on their first day, this should _usually_ not pose a problem.
