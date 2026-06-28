import { getTranslation } from "@/i18n";
import {
  OAuthGrantType,
  OAuthResponseType,
  OAuthTokenEndpointAuthMethod,
  type OAuthGrantType as OAuthGrantTypeValue,
  type OAuthResponseType as OAuthResponseTypeValue,
  type OAuthTokenEndpointAuthMethod as OAuthTokenEndpointAuthMethodValue,
} from "@/services/types";

export const oauthGrantTypeValues = [
  OAuthGrantType.AuthorizationCode,
  OAuthGrantType.ClientCredentials,
  OAuthGrantType.RefreshToken,
] satisfies OAuthGrantTypeValue[];

export const oauthResponseTypeValues = [
  OAuthResponseType.Code,
] satisfies OAuthResponseTypeValue[];

export const oauthTokenEndpointAuthMethodValues = [
  OAuthTokenEndpointAuthMethod.ClientSecretBasic,
  OAuthTokenEndpointAuthMethod.ClientSecretPost,
  OAuthTokenEndpointAuthMethod.None,
] satisfies OAuthTokenEndpointAuthMethodValue[];

export const oauthGrantTypeTranslationKeys: Record<
  OAuthGrantTypeValue,
  string
> = {
  [OAuthGrantType.AuthorizationCode]: "oauthGrantTypes.authorizationCode",
  [OAuthGrantType.ClientCredentials]: "oauthGrantTypes.clientCredentials",
  [OAuthGrantType.RefreshToken]: "oauthGrantTypes.refreshToken",
};

export const oauthResponseTypeTranslationKeys: Record<
  OAuthResponseTypeValue,
  string
> = {
  [OAuthResponseType.Code]: "oauthResponseTypes.code",
};

export const oauthTokenEndpointAuthMethodTranslationKeys: Record<
  OAuthTokenEndpointAuthMethodValue,
  string
> = {
  [OAuthTokenEndpointAuthMethod.ClientSecretBasic]:
    "oauthTokenEndpointAuthMethods.clientSecretBasic",
  [OAuthTokenEndpointAuthMethod.ClientSecretPost]:
    "oauthTokenEndpointAuthMethods.clientSecretPost",
  [OAuthTokenEndpointAuthMethod.None]: "oauthTokenEndpointAuthMethods.none",
};

export function getOAuthGrantTypeLabel(grantType: string) {
  const key =
    oauthGrantTypeTranslationKeys[grantType as OAuthGrantTypeValue] ??
    grantType;

  return getTranslation(key);
}

export function getOAuthResponseTypeLabel(responseType: string) {
  const key =
    oauthResponseTypeTranslationKeys[responseType as OAuthResponseTypeValue] ??
    responseType;

  return getTranslation(key);
}

export function getOAuthTokenEndpointAuthMethodLabel(method: string) {
  const key =
    oauthTokenEndpointAuthMethodTranslationKeys[
      method as OAuthTokenEndpointAuthMethodValue
    ] ?? method;

  return getTranslation(key);
}
