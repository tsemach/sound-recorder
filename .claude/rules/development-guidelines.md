---
trigger: always_on
---

---
description: Development Guidelines
globs: 
alwaysApply: true
---
 
 ## Planning
 - Always plan the code before writing it
 - Think about how the new code will fit into the existing codebase
 - Think about how the new code will interact with other parts of the codebase
 - Think about how the new code will handle errors and edge cases
 - Think about how the new code will be used by the frontend
 - Think about how the new code will be used by the users
 - Think about how the new code will be used by the developers

## File Organization
- Break code into multiple smaller files instead of creating large monolithic files
- Keep files focused on a single responsibility or closely related functionality
- Aim for files under 200-300 lines when possible
- Split large components, services, or modules into logical sub-modules
- Use clear directory structure to organize related files

## Testing Requirements (80/20 Rule)
- Follow the 80/20 rule: cover the main flow and critical business logic
- Include basic failure scenarios, but don't aim for exhaustive edge case coverage
- Follow existing testing patterns and conventions in the codebase
- Ensure tests are deterministic and properly isolated
 
## Typing in case of Typescript
- Always use proper TypeScript types for all variables, parameters, and return values
- Avoid using 'any' type unless absolutely necessary
- Try finding existing types / defined package types, and re-use them or build on top of them instead of creating new ones
- For functions with 3+ parameters, use types (see method-typing.mdc for details)
- All DTO string fields must include @Escape() decorator for security (see dto-validation.mdc)
 
 ## Clean Code
 - Write clean, readable, and maintainable code
 - Keep functions small and focused
 - Use descriptive and clear variable, function, and class names that explain their purpose
 - Avoid code comments by default — prefer self-documenting code with meaningful names
 - Comments are allowed only for complex business logic or non-obvious decisions
 - NEVER delete existing comments; adjust them if the code changes materially
 
 ## Logging and Monitoring
 - Use structured logs, always use contextLogger
 - For metrics, always use the ReporterService from nestjs-metrics-reporter
 - Ensure metrics are collected for key operations

 ## Code Quality
 - DRY (Don't Repeat Yourself): Identify and refactor duplicated code
 - SRP (Single Responsibility Principle): Ensure each module/function has one responsibility
 - Separation of Concerns: Ensure different concerns are handled in separate modules/components
 - Meaningful Names: Verify that names are descriptive and adhere to conventions
 - Parameter Handling: Avoid redundant parameter extraction; keep parameters close to the logic where they are used
 - Local Convention First: Before introducing or changing a pattern (styling, typing, helper placement, React patterns), inspect nearby comparable code in the same package or feature area. Treat adjacent code as the source of truth — do not apply a generic best practice if the touched area already has a consistent convention.
 

