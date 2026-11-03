// SPDX-License-Identifier: MIT
// Retain the pre-member-pointer parse path without maintaining a second grammar fork.
const CPP = require('../tree-sitter-cpp/grammar');

module.exports = grammar(CPP, {
  name: 'cpp_compat',
  rules: {
    _abstract_declarator: ($, original) => {
      const members = original.members.filter(
        member => member.name !== 'abstract_member_pointer_declarator',
      );
      if (members.length !== original.members.length - 1) {
        throw new Error('Expected exactly one abstract member-pointer alternative');
      }
      return choice(...members);
    },
  },
});
