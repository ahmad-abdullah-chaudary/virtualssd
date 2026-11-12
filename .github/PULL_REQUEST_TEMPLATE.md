name: Pull request
description: Propose a change to VirtualSSD.
labels: ["pull request"]
body:
  - type: markdown
    attributes:
      value: |
        Thanks for contributing.

        Please read [CONTRIBUTING.md](https://github.com/ahmad-abdullah-chaudary/virtualssd/blob/main/CONTRIBUTING.md).
        In particular:

        - The UI must reflect backend truth. Never fabricate a value or claim
          a success that did not happen.
        - Never log a secret.
        - A change to the storage path needs a test.

  - type: input
    id: issue
    attributes:
      label: Linked issue
      description: "Closes #"
      placeholder: "Closes #123"
    validations:
      required: false

  - type: textarea
    id: what
    attributes:
      label: What does this change?
    validations:
      required: true

  - type: textarea
    id: why
    attributes:
      label: Why is it needed?
      description: Describe the problem, not just the solution.
    validations:
      required: true

  - type: textarea
    id: tests
    attributes:
      label: How was this tested?
      description: Include the commands you ran and what you observed.
    validations:
      required: true

  - type: textarea
    id: notes
    attributes:
      label: Anything a reviewer should know?
      description: |
        Known limitations, follow-up work, or decisions you want challenged.
