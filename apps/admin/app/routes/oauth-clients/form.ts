import {
  OAuthGrantType,
  OAuthResponseType,
  OAuthTokenEndpointAuthMethod,
  type CreateOAuthClientRequest,
  type JsonValue,
  type OAuthClient,
  type OAuthGrantType as OAuthGrantTypeValue,
  type OAuthResponseType as OAuthResponseTypeValue,
  type OAuthTokenEndpointAuthMethod as OAuthTokenEndpointAuthMethodValue,
  type UpdateOAuthClientRequest,
} from "@/services/types";

type Translate = (key: string) => string;

export interface OAuthClientFormValues {
  clientId: string;
  name: string;
  description: string;
  orgId: string;
  serviceAccountId: string;
  tokenEndpointAuthMethod: OAuthTokenEndpointAuthMethodValue;
  grantTypes: OAuthGrantTypeValue[];
  responseTypes: OAuthResponseTypeValue[];
  redirectUris: string[];
  scopes: string[];
  audiences: string[];
  attrs: string;
}

export const emptyOAuthClientFormValues: OAuthClientFormValues = {
  clientId: "",
  name: "",
  description: "",
  orgId: "",
  serviceAccountId: "",
  tokenEndpointAuthMethod: OAuthTokenEndpointAuthMethod.ClientSecretBasic,
  grantTypes: [],
  responseTypes: [],
  redirectUris: [],
  scopes: [],
  audiences: [],
  attrs: "{}",
};

function cleanList(values: string[]) {
  return values.map((value) => value.trim()).filter(Boolean);
}

function optionalString(value: string) {
  return value.trim() || null;
}

export function oauthClientToFormValues(
  oauthClient?: OAuthClient | null,
): OAuthClientFormValues {
  if (!oauthClient) return emptyOAuthClientFormValues;

  return {
    clientId: oauthClient.clientId,
    name: oauthClient.name,
    description: oauthClient.description ?? "",
    orgId: oauthClient.orgId ?? "",
    serviceAccountId: oauthClient.serviceAccountId ?? "",
    tokenEndpointAuthMethod: oauthClient.tokenEndpointAuthMethod,
    grantTypes: oauthClient.grantTypes,
    responseTypes: oauthClient.responseTypes,
    redirectUris: oauthClient.redirectUris,
    scopes: oauthClient.scopes,
    audiences: oauthClient.audiences,
    attrs: JSON.stringify(oauthClient.attrs ?? {}, null, 2),
  };
}

export function validateOAuthClientForm(t: Translate) {
  return (values: OAuthClientFormValues) => {
    const errors: Record<string, string> = {};
    const grants = new Set(values.grantTypes);
    const responses = new Set(values.responseTypes);

    if (!values.name.trim()) {
      errors.name = t("nameRequired");
    }

    if (values.grantTypes.length === 0) {
      errors.grantTypes = t("grantTypesRequired");
    }

    if (
      grants.has(OAuthGrantType.ClientCredentials) &&
      values.tokenEndpointAuthMethod === OAuthTokenEndpointAuthMethod.None
    ) {
      errors.tokenEndpointAuthMethod = t(
        "clientCredentialsRequiresConfidentialClient",
      );
    }

    if (grants.has(OAuthGrantType.AuthorizationCode)) {
      if (!responses.has(OAuthResponseType.Code)) {
        errors.responseTypes = t("authorizationCodeRequiresCodeResponseType");
      }

      if (cleanList(values.redirectUris).length === 0) {
        errors.redirectUris = t("authorizationCodeRequiresRedirectUri");
      }
    }

    if (
      responses.has(OAuthResponseType.Code) &&
      !grants.has(OAuthGrantType.AuthorizationCode)
    ) {
      errors.responseTypes = t("codeResponseTypeRequiresAuthorizationCode");
    }

    if (
      grants.has(OAuthGrantType.RefreshToken) &&
      !grants.has(OAuthGrantType.AuthorizationCode)
    ) {
      errors.grantTypes = t("refreshTokenRequiresAuthorizationCode");
    }

    if (
      cleanList(values.scopes).includes("offline_access") &&
      !grants.has(OAuthGrantType.RefreshToken)
    ) {
      errors.scopes = t("offlineAccessRequiresRefreshToken");
    }

    try {
      JSON.parse(values.attrs);
    } catch {
      errors.attrs = t("invalidJson");
    }

    return errors;
  };
}

export function toCreateOAuthClientRequest(
  values: OAuthClientFormValues,
): CreateOAuthClientRequest {
  return {
    clientId: optionalString(values.clientId),
    name: values.name.trim(),
    description: optionalString(values.description),
    orgId: optionalString(values.orgId),
    serviceAccountId: optionalString(values.serviceAccountId),
    tokenEndpointAuthMethod: values.tokenEndpointAuthMethod,
    grantTypes: values.grantTypes,
    responseTypes: values.responseTypes,
    redirectUris: cleanList(values.redirectUris),
    scopes: cleanList(values.scopes),
    audiences: cleanList(values.audiences),
    attrs: JSON.parse(values.attrs) ?? {},
  };
}

export function toUpdateOAuthClientRequest(
  values: OAuthClientFormValues,
  attrs: JsonValue,
): UpdateOAuthClientRequest {
  return {
    name: values.name.trim(),
    description: optionalString(values.description),
    orgId: optionalString(values.orgId),
    serviceAccountId: optionalString(values.serviceAccountId),
    tokenEndpointAuthMethod: values.tokenEndpointAuthMethod,
    grantTypes: values.grantTypes,
    responseTypes: values.responseTypes,
    redirectUris: cleanList(values.redirectUris),
    scopes: cleanList(values.scopes),
    audiences: cleanList(values.audiences),
    attrs,
  };
}
