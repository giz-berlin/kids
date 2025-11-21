# James Target

Synchronizes source groups to users, lists and teams in James.

## Working principles

### Group syncing

For syncing source groups to James there two attribute with the keys `james-team` and `james-list` having email addresses as values. Only source groups including key value pairs with this keys will be synced. Each source group has an uuid.

If the `james-team` is set, we create a James team with `uuid` as team name under the domain specified in `james_team_domain`. Additionally, we create an alias email address for the team email address for each email in the `james-team` attribute.

If the `james-list` is set, we do not need to create something because list are created by adding a first user. But we create an alias email address for the list email address (uuid@DOMAIN, where DOMAIN has the value of `james_list_domain`) for each email in the `james-list` attribute.

If the values of the `james-team` or the `james-list` attribute is created, changed, or deleted; this update is reflected in James.

Source group membership will not be handled via the group sync.

If the `james-team` or `james-list` attribute is removed from a source group, we delete all members and aliases of the team or list. For a team we will not delete the team mailbox. 

If a source group is removed we will do the same. Additionally, we will remove the team mailbox here.

### User syncing

All source user will be synced to James. Each user in the source has an uuid. Initially we create for each source user a user in James with an email uuid@DOMAIN. The domain takes the value of the `james_user_domain` config variable. Additionally, there will be created a mailbox for this email named "INBOX". 

A source user can have attributes that are key value pairs. The key `james-alias` should have an email as value. If this key is set for a source user, we create for this user in James an alias email address with the email specified in the value. There could be multiple attributes with the `james-alias` key, resulting in multiple alias addresses for a user in James. 

Furthermore, there is a field named `email` in a source user. The value will be synchronized as alias email address as well. 

If the value of the `email` field or the `james-alias` attribute is created, changed, or deleted; this update is reflected in James.

If user is deleted in source we also delete the user and its mailbox in James as well as all its alias addresses.

A source user has also a groups field. With this field the user's group membership is managed. We update the memberships according to that field in James. If the first user is added to a list, the list is automatically created. List have the form of uuid@DOMAIN, where DOMAIN has the value of `james_list_domain`. If the last user of a list is removed, the list is automatically deleted. 

### Email address domains

The Email addresses for users, lists and teams have special domains specified in the following config values:

- james_user_domain
- james_list_domain
- james_team_domain

These domains should not be used for alias addresses in James. Because of that we do not create an alias address if the `james-alias`, `james-team` or `james-list` attribute include an email with this domain.

## Configuration

Please refer to the [configuration example file](../../../default_configs/james-target.config.example.toml).