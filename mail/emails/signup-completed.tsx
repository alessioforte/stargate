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

export const SignupCompleted = () => {
  return (
    <Html>
      <Tailwind>
        <Head />
        <Body style={main}>
          <Preview>Signup Completed</Preview>
          <Container className="pt-10">
            <Section>
              <Row className="p-5">
                <Column>
                  <Heading className="text-[32px] font-bold text-center">
                    Signup Completed
                  </Heading>

                  <Text className="text-base">
                    Your account has been successfully created. You can now log
                    in using your email address and password.
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

export default SignupCompleted;

const main = {
  fontFamily:
    "-apple-system, BlinkMacSystemFont, 'Segoe UI', 'Roboto', 'Oxygen', 'Ubuntu', 'Cantarell', 'Fira Sans', 'Droid Sans', 'Helvetica Neue', sans-serif",
};
