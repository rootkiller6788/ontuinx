package com.example.model;

public class User implements Entity {
    private String id;
    private String name;
    private String email;
    private long version;

    public User(String id, String name, String email) {
        this.id = id;
        this.name = name;
        this.email = email;
    }

    @Override public String getId() { return id; }
    @Override public long getVersion() { return version; }
    public String getName() { return name; }
    public String getEmail() { return email; }
    public void setName(String name) { this.name = name; }
}
