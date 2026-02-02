import {
  Body,
  Button,
  Column,
  Container,
  Head,
  Heading,
  Html,
  Preview,
  Row,
  Section,
  Tailwind,
  Text,
} from "@react-email/components";

export const PasswordChanged = () => {
  return (
    <Html>
      <Tailwind>
        <Head />
        <Body style={main}>
          <Preview>Password Changed</Preview>
          <Container className="pt-10">
            <Section>
              <Row className="p-5">
                <Column>
                  <Heading className="text-[32px] font-bold text-center">
                    Password Changed
                  </Heading>

                  <Text className="text-base">
                    Your password has been changed successfully.
                  </Text>
                  <Text className="text-base">
                    If you did not request a password change, please contact us
                    immediately.
                  </Text>
                </Column>
              </Row>
            </Section>
            <Text className="text-center text-xs leading-[24px] text-black/70">
              © {new Date().getFullYear()} | Stargate Inc. All rights reserved.
            </Text>
          </Container>
        </Body>
      </Tailwind>
    </Html>
  );
};

export default PasswordChanged;

const main = {
  fontFamily:
    "-apple-system, BlinkMacSystemFont, 'Segoe UI', 'Roboto', 'Oxygen', 'Ubuntu', 'Cantarell', 'Fira Sans', 'Droid Sans', 'Helvetica Neue', sans-serif",
};
