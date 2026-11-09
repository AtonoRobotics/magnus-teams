The console and Keycloak test helpers time out after two minutes. A hung request now fails its test and names itself, where before the runner killed the test after 600 s with nothing to show (#239).
