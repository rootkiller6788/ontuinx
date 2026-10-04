package com.example.service;

import com.example.model.User;
import com.example.repo.UserRepository;
import java.util.Optional;
import java.util.List;

public class UserService {
    private final UserRepository repo;
    private final AuditLogger audit;

    public UserService(UserRepository repo, AuditLogger audit) {
        this.repo = repo;
        this.audit = audit;
    }

    public Optional<User> getUser(String id) {
        audit.log("getUser", id);
        return repo.findById(id);
    }

    public List<User> listUsers() {
        audit.log("listUsers", "");
        return repo.findAll();
    }

    public User createUser(String name, String email) {
        String id = java.util.UUID.randomUUID().toString();
        User user = new User(id, name, email);
        audit.log("createUser", id);
        return repo.save(user);
    }
}

interface AuditLogger {
    void log(String action, String target);
}
