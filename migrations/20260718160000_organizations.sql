CREATE SCHEMA IF NOT EXISTS organizations;

CREATE TABLE organizations.organizations (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    slug TEXT NOT NULL,
    created_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT organizations_slug_normalized CHECK (
        slug = lower(slug)
        AND slug ~ '^[a-z0-9]([a-z0-9-]{1,61}[a-z0-9])$'
    )
);

CREATE UNIQUE INDEX organizations_slug_unique_idx
    ON organizations.organizations (slug);

CREATE TABLE organizations.memberships (
    organization_id UUID NOT NULL
        REFERENCES organizations.organizations(id) ON DELETE CASCADE,
    user_id UUID NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member')),
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (organization_id, user_id)
);

CREATE INDEX organizations_memberships_user_idx
    ON organizations.memberships (user_id, joined_at DESC);
CREATE INDEX organizations_memberships_org_role_idx
    ON organizations.memberships (organization_id, role);

CREATE TABLE organizations.invitations (
    id UUID PRIMARY KEY,
    organization_id UUID NOT NULL
        REFERENCES organizations.organizations(id) ON DELETE CASCADE,
    email TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member')),
    token_hash TEXT NOT NULL UNIQUE CHECK (char_length(token_hash) = 64),
    expires_at TIMESTAMPTZ NOT NULL,
    accepted_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    invited_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT organizations_invitations_email_normalized CHECK (email = lower(email)),
    CONSTRAINT organizations_invitations_state CHECK (
        accepted_at IS NULL OR revoked_at IS NULL
    )
);

CREATE INDEX organizations_invitations_org_idx
    ON organizations.invitations (organization_id, created_at DESC);
CREATE INDEX organizations_invitations_email_active_idx
    ON organizations.invitations (email, expires_at)
    WHERE accepted_at IS NULL AND revoked_at IS NULL;

COMMENT ON SCHEMA organizations IS
    'Organizations, memberships, and single-use invitations';
COMMENT ON COLUMN organizations.organizations.created_by IS
    'Soft identity user id; identity owns lifecycle and referential policy';
COMMENT ON COLUMN organizations.memberships.user_id IS
    'Soft identity user id; no cross-capability foreign key';
COMMENT ON COLUMN organizations.invitations.invited_by IS
    'Soft identity user id; retained for audit history';
