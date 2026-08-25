window.onload = function () {
  // Add event listeners to all "Add Row" buttons
  document.querySelectorAll(".add-row").forEach((button) => {
    button.addEventListener("click", function () {
      const tableId = this.getAttribute("data-table");
      console.log("table id", tableId);
      const table = document.getElementById(tableId);
      const templateRow = table.querySelector(".template");

      // Clone the template row
      const newRow = templateRow.cloneNode(true);
      newRow.classList.remove("template");

      // Remove the 'form' attribute from all inputs in the new row
      newRow.querySelectorAll("input").forEach((input) => {
        input.removeAttribute("form");
      });

      // Append the new row to the table
      table.appendChild(newRow);

      // Add event listener to the new "Remove" button
      newRow.querySelector(".remove").addEventListener("click", function () {
        newRow.remove();
      });
    });
  });

  // Add event listeners to all existing "Remove" buttons
  document.querySelectorAll(".remove").forEach((button) => {
    button.addEventListener("click", function () {
      this.closest("tr").remove();
    });
  });

  // Copy placeholder helper buttons
  document.querySelectorAll(".copy-placeholder").forEach((button) => {
    button.addEventListener("click", function () {
      const row = this.closest("tr");
      const codeNode = row ? row.querySelector("code") : null;
      const value = codeNode ? codeNode.textContent || "" : "";
      if (!value) {
        return;
      }
      navigator.clipboard.writeText(value);
    });
  });

  // Clickable table rows
  document.querySelectorAll(".clickable-row").forEach((row) => {
    const href = row.getAttribute("data-href");
    if (!href) {
      return;
    }

    row.addEventListener("click", function () {
      window.location.href = href;
    });

    row.addEventListener("keydown", function (event) {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        window.location.href = href;
      }
    });
  });

  // Remember the last import mapping in this browser.
  const importMappingForm = document.querySelector("[data-import-mapping]");
  if (importMappingForm) {
    const storageKey = "birthday-mail-sender.import-mapping.v1";
    const fieldNames = [
      "first_name",
      "last_name",
      "greeting",
      "email",
      "birthday",
    ];

    const selectHasValue = (select, value) =>
      Array.from(select.options).some((option) => option.value === value);

    try {
      const savedMapping = JSON.parse(localStorage.getItem(storageKey) || "{}");
      fieldNames.forEach((fieldName) => {
        [fieldName, `${fieldName}_transform`].forEach((selectName) => {
          const select = importMappingForm.elements.namedItem(selectName);
          const savedValue = savedMapping[selectName];
          if (
            select instanceof HTMLSelectElement &&
            typeof savedValue === "string" &&
            selectHasValue(select, savedValue)
          ) {
            select.value = savedValue;
          }
        });
      });
    } catch (_error) {
      // Ignore unavailable storage and malformed saved values.
    }

    importMappingForm.addEventListener("submit", function () {
      const mapping = {};
      fieldNames.forEach((fieldName) => {
        [fieldName, `${fieldName}_transform`].forEach((selectName) => {
          const select = importMappingForm.elements.namedItem(selectName);
          if (select instanceof HTMLSelectElement) {
            mapping[selectName] = select.value;
          }
        });
      });

      try {
        localStorage.setItem(storageKey, JSON.stringify(mapping));
      } catch (_error) {
        // The import should still proceed when storage is unavailable.
      }
    });

    const resetButton = document.querySelector("[data-reset-import-mapping]");
    if (resetButton) {
      resetButton.addEventListener("click", function () {
        try {
          localStorage.removeItem(storageKey);
        } catch (_error) {
          // Reset the visible form even when storage is unavailable.
        }

        fieldNames.forEach((fieldName) => {
          const columnSelect = importMappingForm.elements.namedItem(fieldName);
          const transformSelect = importMappingForm.elements.namedItem(
            `${fieldName}_transform`,
          );
          if (columnSelect instanceof HTMLSelectElement) {
            columnSelect.value = "";
          }
          if (transformSelect instanceof HTMLSelectElement) {
            transformSelect.value = "none";
          }
        });
      });
    }
  }
};
