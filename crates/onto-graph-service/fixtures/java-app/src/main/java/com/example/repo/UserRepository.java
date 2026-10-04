package com.example.repo;

import com.example.model.User;
import java.util.Map;
import java.util.HashMap;
import java.util.Optional;
import java.util.List;
import java.util.ArrayList;

public class UserRepository {
    private final Map<String, User> store = new HashMap<>();

    public Optional<User> findById(String id) {
        return Optional.ofNullable(store.get(id));
    }

    public List<User> findAll() {
        return new ArrayList<>(store.values());
    }

    public User save(User user) {
        store.put(user.getId(), user);
        return user;
    }

    public void deleteById(String id) {
        store.remove(id);
    }
}
