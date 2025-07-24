import yaml
import os

CONFIG_FILE = 'synapse_data/homeserver.yaml'

with open(CONFIG_FILE) as f:
    config = yaml.safe_load(f)
    # See https://element-hq.github.io/synapse/latest/usage/configuration/config_documentation.html
    config['listeners'].append({
        'port': int(os.environ.get('SYNAPSE_TLS_PORT')),
        'type': 'http',
        'tls': True,
        'resources': [{
            'names': ['client', 'federation']
        }]
    })
    config['tls_certificate_path'] = f'/opt/ca/{os.environ.get("SYNAPSE_CONTAINER_NAME")}.crt'
    config['tls_private_key_path'] = f'/opt/ca/{os.environ.get("SYNAPSE_CONTAINER_NAME")}.key'
    config['oidc_providers'] = [{
        'idp_id': 'keycloak',
        'idp_name': 'Keycloak',
        'issuer': f'https://{os.environ.get("PODMAN_SERVICE_HOSTNAME")}:8443/realms/giz',
        'client_id': 'synapse',
        'client_secret': 'synapse_secret',
        'scopes': ['openid', 'profile'],
        'user_mapping_provider': {
            'config': {
                'local_part_template': '{{ user.preferred_username }}',
                'display_name_template': '{{ user.name }}'
            }
        }
    }]
    config['rc_login'] = {
        'address': {
            'per_second': 5,
            'burst_count': 20
        },
        'account': {
            'per_second': 5,
            'burst_count': 20
        }
    }
    config['refreshable_access_token_lifetime'] = '30s'

with open(CONFIG_FILE, 'w') as f:
    yaml.dump(config, f, default_flow_style=False, sort_keys=False)
